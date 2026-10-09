from pathlib import Path
import sys,json,hashlib,importlib.util,tempfile,zipfile,stat,warnings
sys.dont_write_bytecode=True
root=Path(__file__).resolve().parent
out=root/"receiver-v2-controls"
out.mkdir(exist_ok=False)
spec=importlib.util.spec_from_file_location("intake_v2",root/"intake_once.v2.py")
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
results=[]
def ok(name):results.append({"name":name,"passed":True})
def rejection(name,fn,kind=ValueError):
    try:fn()
    except kind as error:results.append({"name":name,"passed":True,"error":str(error)})
    else:raise AssertionError(name+" did not fail closed")
rejection("pending_binding_cannot_execute",lambda:m.checked_binding(root/"receiver-binding.template.json"))
run={"id":1,"head_sha":"a"*40,"run_attempt":1,"event":"workflow_dispatch","path":".github/workflows/p8-scale.yml","head_branch":"x"}
binding={"run_id":1,"source_G":"a"*40,"expected_run_attempt":1,"branch":"x"}
m.verify_run(run,binding);ok("exact_run_binding_accepted")
rejection("wrong_head_rejected",lambda:m.verify_run(dict(run,head_sha="b"*40),binding))
rejection("second_attempt_rejected",lambda:m.verify_run(dict(run,run_attempt=2),binding))
with tempfile.TemporaryDirectory(prefix="p8-intake-v2-synthetic-") as tmp:
    t=Path(tmp);zpath=t/"valid.zip"
    with zipfile.ZipFile(zpath,"w") as z:z.writestr("native/raw.jsonl",b'{"synthetic_transport_control":true}\n')
    before=m.sha(zpath);members=m.extract_original(zpath,t/"valid-extracted")
    assert m.sha(zpath)==before and len(members)==1 and (t/"valid-extracted/native/raw.jsonl").read_bytes()==b'{"synthetic_transport_control":true}\n'
    ok("safe_zip_exact_bytes_crc_hash_and_original_unchanged")
    for name,kind in [("parent_traversal","traversal"),("absolute_path","absolute"),("duplicate_member","duplicate"),("symlink","symlink")]:
        p=t/(name+".zip")
        with warnings.catch_warnings():
            warnings.simplefilter("ignore",UserWarning)
            with zipfile.ZipFile(p,"w") as z:
                if kind=="traversal":z.writestr("../escape",b"x")
                elif kind=="absolute":z.writestr("/escape",b"x")
                elif kind=="duplicate":
                    z.writestr("x",b"1");z.writestr("x",b"2")
                else:
                    zi=zipfile.ZipInfo("link");zi.create_system=3;zi.external_attr=(stat.S_IFLNK|0o777)<<16;z.writestr(zi,b"../escape")
        def check():
            with zipfile.ZipFile(p) as z:list(m.validate_members(z))
        rejection(name+"_rejected",check)
    nul=t/"nul.zip"
    with zipfile.ZipFile(nul,"w") as z:z.writestr("abcXdef",b"payload")
    original=nul.read_bytes();assert original.count(b"abcXdef")==2
    nul.write_bytes(original.replace(b"abcXdef",b"abc\0def"))
    with zipfile.ZipFile(nul) as z:
        info=z.infolist()[0];assert info.filename=="abc" and info.orig_filename=="abc\0def"
        rejection("NUL_orig_filename_rejected",lambda:list(m.validate_members(z)))
    meta={"size_in_bytes":zpath.stat().st_size,"digest":"sha256:"+before}
    for name,modified,fail in [
        ("size_and_digest_agree",meta,False),
        ("size_mismatch_even_with_matching_digest",dict(meta,size_in_bytes=meta["size_in_bytes"]+1),True),
        ("size_mismatch_without_digest",dict(meta,size_in_bytes=meta["size_in_bytes"]+1,digest=None),True),
        ("digest_mismatch_with_matching_size",dict(meta,digest="sha256:"+"0"*64),True),
        ("missing_digest_is_explicitly_unverified",dict(meta,digest=None),False)]:
        dest=out/name;dest.mkdir()
        if fail:rejection(name,lambda:m.verify_zip_identity(zpath,modified,dest))
        else:
            got,verified=m.verify_zip_identity(zpath,modified,dest)
            assert got==before and verified==(modified["digest"] is not None)
            ok(name)
        assert (dest/"zip-receipt.json").is_file() and m.sha(zpath)==before and not (dest/"extracted").exists()
    fake=t/"synthetic-api"
    fake.write_text("#!"+sys.executable+"\nimport os,sys\nos.write(1,b'{\"synthetic\":true}\\n')\nos.write(2,bytes([255,0,254])+b'stderr\\n')\nsys.exit(7 if sys.argv[-1]=='/failure' else 0)\n")
    fake.chmod(0o700)
    real_gh=m.GH;m.GH=str(fake)
    try:
        for endpoint,expected_exit in [("/success",0),("/failure",7)]:
            calls=[];dest=out/("synthetic-api-"+str(expected_exit)+".json")
            if expected_exit==0:
                assert m.api(endpoint,dest,calls)=={"synthetic":True}
            else:rejection("API_failure_preserves_actual_exit7",lambda:m.api(endpoint,dest,calls),RuntimeError)
            stderr=dest.with_name(dest.name+".stderr")
            assert stderr.read_bytes()==bytes([255,0,254])+b"stderr\n"
            assert dest.read_bytes()==b'{"synthetic":true}\n'
            assert calls[0]["exit_code"]==expected_exit
            (out/("synthetic-api-"+str(expected_exit)+"-calls.json")).write_text(json.dumps(calls,indent=2)+"\n")
            if expected_exit==0:ok("API_stderr_original_nonUTF8_NUL_bytes_preserved")
    finally:m.GH=real_gh
report={"scope":"Synthetic transport controls only. GH executable replaced only in this test module by an owned fixture that never contacts a network. No source/validator/measurement command modified or run.","receiver_sha256":m.sha(root/"intake_once.v2.py"),"passed":len(results),"failed":0,"controls":results,"preparation_script_completed_without_exception":True}
(out/"results.json").write_text(json.dumps(report,indent=2)+"\n")
print(json.dumps({"passed":len(results),"failed":0,"scope":"synthetic transport only"}))

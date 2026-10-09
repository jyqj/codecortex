"""Automate the previously accepted lifecycle Cargo selection addendum.

Caller binds original archive, complete source, binary digests and seven observers.
This only compares original receipts and logs; it never builds a native product.
"""
from pathlib import PurePosixPath

RELEASE_PROFILE=dict(debug_assertions=False,debuginfo=0,opt_level='3',overflow_checks=False,test=False)


def verify_original_cargo(product,replay,product_messages,replay_messages):
    a=product['cargo_artifact'];b=replay['artifact']
    producer=PurePosixPath(a['manifest_path']).parents[2]
    target=PurePosixPath(product['cargo_target_dir'])
    assert producer.is_absolute() and '..' not in producer.parts
    assert target.is_absolute() and '..' not in target.parts
    assert product['package_kind']=='default' and product['build_profile']=='release'
    assert product['cargo_target_initially_absent'] is True
    assert product['cargo_build_dir']==str(target)
    assert product['binary_path']==str(target.parent/'codecortex')
    assert product['actual_cargo_profile']==a['profile']==b['profile']==RELEASE_PROFILE
    assert product['source_before']==product['source_after']==replay['source_before']==replay['source_after']
    assert product['build_exit_code']==replay['build_exit_code']==0
    assert product['build_command']==['cargo','build','-p','cc-server','--bin','codecortex','--no-default-features','--locked','--message-format=json-render-diagnostics','--offline','--release']
    assert replay['build_command']==['cargo','build','--offline','--locked','--release','-p','cc-eval','--bin','p8-measurements','--target-dir',str(target),'--message-format=json-render-diagnostics']
    for artifact,package,name,src,features,rows in [
        (a,'cc-server','codecortex','src/main.rs',[[]],product_messages),
        (b,'cc-eval','p8-measurements','src/bin/p8-measurements.rs',[[],['default']],replay_messages),
    ]:
        assert artifact['reason']=='compiler-artifact' and artifact['fresh'] is False
        assert artifact['features'] in features
        assert artifact['manifest_path']==str(producer/'crates'/package/'Cargo.toml')
        assert artifact['target']['name']==name and artifact['target']['kind']==artifact['target']['crate_types']==['bin']
        assert artifact['target']['src_path']==str(producer/'crates'/package/src)
        assert artifact['package_id'].rpartition('#')[0]=='path+file://'+str(producer/'crates'/package)
        assert artifact['executable']==str(target/'release'/name)
        assert [r for r in rows if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==name]==[artifact]
        assert rows[-1]=={'reason':'build-finished','success':True}
        assert [r for r in rows if r.get('reason')=='build-finished']==[{'reason':'build-finished','success':True}]
    assert replay['copy_source']['path']==b['executable']
    assert replay['copy_source']['sha256']==replay['binary_sha256']
    return dict(original_product_unique_artifact_equals_receipt=True,original_evaluator_unique_artifact_equals_receipt=True,
                both_successful_build_finished_last=True,exact_original_commands=True,same_original_producer=True,
                same_private_target=True,actual_release_profile=RELEASE_PROFILE,product_features=a['features'],evaluator_features=b['features'],
                scope='Exact original Cargo log/receipt selection and same producer/private target. No invented source_root field or hermetic binary certification.')

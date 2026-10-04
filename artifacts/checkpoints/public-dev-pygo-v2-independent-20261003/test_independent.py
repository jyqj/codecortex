"""Self-authored grammar, runtime-type, and fail-closed mutation tests."""
import copy, json, types, unittest
import audit
class IndependentTests(unittest.TestCase):
    def test_python_owner_boundaries(self):
        src=b'''# def phantom(): pass
class Vessel:
    @staticmethod
    def ping(): pass
    @classmethod
    def make(cls): pass
    @property
    def value(self): return 1
    @value.setter
    def value(self, v): pass
    def run(self):
        def ping(): pass
        class Inner:
            def ping(self): pass
        return ping
    if True:
        async def awaitable(self): pass
class Other:
    def ping(self): pass
def free():
    class Local:
        def ping(self): pass
    def nested(): pass
text="def phantom(): pass"
'''
        defs=audit.python_decls(src);m={d['scope']:d['kind'] for d in defs}
        self.assertEqual(m,{'Vessel':'class','Vessel.ping':'method','Vessel.make':'method','Vessel.value':'method','Vessel.run':'method','Vessel.run.ping':'function','Vessel.run.Inner':'class','Vessel.run.Inner.ping':'method','Vessel.awaitable':'method','Other':'class','Other.ping':'method','free':'function','free.Local':'class','free.Local.ping':'method','free.nested':'function'})
        self.assertEqual(len([d for d in defs if d['scope']=='Vessel.value']),2)
    def test_runtime_type_is_separate_from_declaration(self):
        class Vessel:
            @staticmethod
            def ping(): pass
            @classmethod
            def make(cls): pass
            def run(self): pass
            @property
            def value(self):return 1
        self.assertIsInstance(Vessel.ping,types.FunctionType)
        self.assertIsInstance(Vessel.make,types.MethodType)
        self.assertIsInstance(Vessel().run,types.MethodType)
        self.assertIsInstance(Vessel.__dict__['value'],property)
        self.assertIsInstance(Vessel.__dict__['value'].fget,types.FunctionType)
    def test_python_utf8_coordinates(self):
        src='label="汉字"\nclass Vessel:\n    def ping(self): return "é"\n'.encode()
        d=next(d for d in audit.python_decls(src) if d['name']=='ping')
        self.assertEqual(src[d['start']:d['end']],b'def ping(self): return "\xc3\xa9"')
    def test_go_grammar_receivers_and_controls(self):
        src=b'''package miniature
// func (v *Wrong) Ping() {}
type Vessel struct{}
type Other struct{}
type Face interface{ Ping() }
var label="func (x *Wrong) Ping() {}"
func (v Vessel) Ping() {}
func (v *Other) Ping() {}
func Free() { f := func() {}; _ = f }
'''
        ds=audit.go_decls(src);functions=[d for d in ds if d['kind'] in ('function','method')]
        self.assertEqual([(d['name'],d['owner'],d['kind']) for d in functions],[('Ping','Vessel','method'),('Ping','Other','method'),('Free','','function')])
        self.assertEqual(sum(d['kind']=='interface' for d in ds),1)
        for d in functions:self.assertTrue(src[d['start']:d['end']].startswith(b'func '))
    def test_json_utf8_and_duplicate_fields(self):
        raw=b'{"label":"\xe6\xb1\x89","symbol": {"kind" : "function"}}\r\n';r=audit.ranges(raw)
        self.assertEqual(raw[slice(*r[('symbol','kind')])],b'"function"')
        with self.assertRaises(ValueError):audit.ranges(b'{"x":1,"x":2}')
    def items(self):
        return [dict(repo='mini',query_ordinal=0,group_ordinal=0,alternative_ordinal=i,owner='owner'+str(i),source_sha256='pinned') for i in range(2)]
    def check_bad(self,mutate):
        originals=self.items();changes=[{'binding':copy.deepcopy(r)} for r in originals];mutate(changes)
        with self.assertRaises(ValueError):audit.validate_changes(changes,audit.unique(originals))
    def test_reject_subset(self):self.check_bad(lambda c:c.pop())
    def test_reject_extra(self):self.check_bad(lambda c:c.append({'binding':dict(c[0]['binding'],alternative_ordinal=3)}))
    def test_reject_duplicate(self):self.check_bad(lambda c:c.append(copy.deepcopy(c[0])))
    def test_reject_forged_owner(self):self.check_bad(lambda c:c[0]['binding'].update(owner='forged'))
    def test_reject_forged_source_pin(self):self.check_bad(lambda c:c[0]['binding'].update(source_sha256='forged'))
    def test_reject_relabel_other_fields(self):
        original=dict(path='mini.py',span=dict(start=3,end=18),symbol=dict(name='ping',qname='Mini.ping',kind='function'))
        valid=copy.deepcopy(original);valid['symbol']['kind']='method'
        audit.verify_overlay(original,json.dumps(valid))
        for field in ('name','qname','kind'):
            bad=copy.deepcopy(valid);bad['symbol'][field]='forged'
            with self.assertRaises(ValueError):audit.verify_overlay(original,json.dumps(bad))
        for bad in (dict(valid,path='other.py'),dict(valid,span=dict(start=4,end=18)),dict(valid,weight=2)):
            with self.assertRaises(ValueError):audit.verify_overlay(original,json.dumps(bad))
    def test_reject_dtype_wildcard_alias(self):
        original=dict(path='mini.py',symbol=dict(name='ping',kind='function'))
        for kind in ('*','callable','function|method','FunctionType'):
            bad=copy.deepcopy(original);bad['symbol']['kind']=kind
            with self.assertRaises(ValueError):audit.verify_overlay(original,json.dumps(bad))
if __name__=='__main__':unittest.main(verbosity=2)

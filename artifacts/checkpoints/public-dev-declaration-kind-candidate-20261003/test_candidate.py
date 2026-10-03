"""Self-authored boundary and rejection witnesses; no current product as truth."""
import copy
import json
import subprocess
import unittest

import candidate as c

PYTHON = '''# 自编，非 benchmark 样本
def outer():
    def inner():
        return 1
    class Local:
        async def tick(self):
            return 2
    return inner, Local

class Compass:
    def turn(self):
        def inner():
            return 3
        return inner()
    @staticmethod
    def reset():
        return 4
    @classmethod
    def build(cls):
        return cls()
    @property
    def bearing(self):
        return 5
    @bearing.setter
    def bearing(self, value):
        pass
    @bearing.deleter
    def bearing(self):
        pass
'''.encode()

GO = '''package witness
// 船: func (x Fake) Noise() {} is text.
type Boat struct{}
type Gauge struct{}
var decoy = "func (x Fake) Noise() {}"
func Free() {}
func (b Boat) Move() {}
func (b *Boat) Tune() {}
func (g Gauge) Move() {}
'''.encode()

def witness():
    # Deliberate spaces, escaping, CRLF, Unicode, decoys and non-kind metadata.
    row = dict(id='自编', query='literal "function" with \\u0061', language='python',
        answers=[dict(id='g', primary=True, weight=0.7, alternatives=[dict(
            path='mini.py', span={'start':5,'end':42},
            symbol=dict(name='reset', qname='mini.Compass.reset',kind='function'))])],
        annotations={'facets':[{'group_id':'g','weight':0.3}]}, threshold=0.81)
    line = ('  '+json.dumps(row, ensure_ascii=False, separators=(', ', ' : '))+'\r\n').encode()
    _, alt, _ = c.alt_ranges(line.decode(),0,0)
    p = dict(repo='mini',query_ordinal=0,group_ordinal=0,alternative_ordinal=0,
        original_alternative_raw_sha256=c.sha(line.decode()[alt[0]:alt[1]].encode()),
        original_kind='function',declaration_kind='method',
        source_entry='pinned:mini.py',source_sha256='self-authored',
        owner_reviews=[{'entry':'pinned:owner.json','row_sha256':'self-authored','decision':'accept'}])
    return line, p

class CandidateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.audit = c.load_review()

    def test_python_declaration_boundaries(self):
        defs = self.audit.python_definitions(PYTHON)
        self.assertEqual([(d['scope'],d['kind'],d['subtype']) for d in defs], [
            ('outer','function','function'),('outer.inner','function','function'),
            ('outer.Local','class','class'),('outer.Local.tick','method','instance_method'),
            ('Compass','class','class'),('Compass.turn','method','instance_method'),
            ('Compass.turn.inner','function','function'),('Compass.reset','method','static_method'),
            ('Compass.build','method','class_method'),
            ('Compass.bearing','method','property_accessor'),
            ('Compass.bearing','method','property_accessor'),
            ('Compass.bearing','method','property_accessor')])
        for d in defs:
            self.assertTrue(PYTHON[d['start']:d['end']].startswith((b'def ', b'async def ', b'class ')))

    def test_go_receiver_owner_and_utf8(self):
        actual = json.loads(subprocess.check_output([str(c.HERE/'.scratch/go-taxonomy')],
            input=json.dumps(GO.decode()).encode()))
        self.assertEqual([(d['name'],d['kind'],d['receiver']) for d in actual],
            [('Free','function',''),('Move','method','Boat'),('Tune','method','Boat'),('Move','method','Gauge')])
        for d in actual:
            self.assertTrue(GO[d['start']:d['end']].startswith(b'func '))

    def test_literal_kind_only_roundtrip(self):
        line, p = witness()
        after = c.apply_line(line,[p])
        self.assertEqual(after, line.replace(b'"kind" : "function"', b'"kind" : "method"'))
        self.assertTrue(after.endswith(b'\r\n'))
        self.assertIn('自编'.encode(),after)
        self.assertIn(b'literal \\"function\\"',after)

    def test_wrong_owner_is_rejected(self):
        _, p = witness()
        wrong = copy.deepcopy(p)
        wrong['owner_reviews'][0]['entry'] = 'pinned:wrong-owner.json'
        with self.assertRaisesRegex(ValueError,'binding drift'):
            c.check_proposals([wrong],[p])

    def test_wrong_source_binding_is_rejected(self):
        _, p = witness()
        for key in ['source_entry','source_sha256']:
            wrong = copy.deepcopy(p)
            wrong[key] = 'other'
            with self.assertRaisesRegex(ValueError,'binding drift'):
                c.check_proposals([wrong],[p])

    def test_missing_is_rejected(self):
        _, p = witness()
        with self.assertRaisesRegex(ValueError,'missing'):
            c.check_proposals([],[p])

    def test_duplicate_is_rejected(self):
        line, p = witness()
        with self.assertRaisesRegex(ValueError,'duplicate proposal'):
            c.check_proposals([p,p],[p])
        with self.assertRaisesRegex(ValueError,'duplicate delta'):
            c.apply_line(line,[p,p])

    def test_extra_is_rejected(self):
        _, p = witness()
        extra = dict(p,alternative_ordinal=1)
        with self.assertRaisesRegex(ValueError,'extra proposal'):
            c.check_proposals([p,extra],[p])

    def test_false_correction_is_rejected(self):
        line, p = witness()
        # A class or already-method annotation is never eligible for this patch.
        for kind in ['class','method']:
            other = line.replace(b'"kind" : "function"',b'"kind" : '+json.dumps(kind).encode())
            _, alt, _ = c.alt_ranges(other.decode(),0,0)
            wrong = dict(p,original_alternative_raw_sha256=c.sha(other.decode()[alt[0]:alt[1]].encode()))
            with self.assertRaisesRegex(ValueError,'false correction'):
                c.apply_line(other,[wrong])
        # A source-backed free function control cannot enter the fixed method set.
        with self.assertRaisesRegex(ValueError,'binding drift'):
            c.check_proposals([dict(p,declaration_kind='function')],[p])

    def test_before_raw_hash_drift_is_rejected(self):
        line, p = witness()
        with self.assertRaisesRegex(ValueError,'alternative binding'):
            c.apply_line(line,[dict(p,original_alternative_raw_sha256='0'*64)])

    def test_non_kind_delta_drift_is_rejected(self):
        line, p = witness()
        after = c.apply_line(line,[p])
        expected = {'gold':after, 'manifest':c.encoded({'delta':{'start':5,'end':15},'owner':'owner'})}
        # Independent stored replay outputs include all non-kind fields and deltas.
        for old,new in [(b'mini.py',b'other.py'),(b'0.7',b'0.9'),(b'true',b'false'),
                        (b'0.81',b'0.82'),(b'python',b'go'),(b'42',b'43'),
                        (b'mini.Compass.reset',b'mini.Other.reset'),(b'group_id',b'facet_id'),
                        (b'literal',b'changed-query')]:
            self.assertIn(old,after)
            with self.assertRaisesRegex(ValueError,'output drift'):
                c.check_candidate_outputs(dict(expected,gold=after.replace(old,new)),expected)
        drift = copy.deepcopy(expected)
        drift['manifest'] = c.encoded({'delta':{'start':6,'end':15},'owner':'owner'})
        with self.assertRaisesRegex(ValueError,'output drift'):
            c.check_candidate_outputs(drift,expected)

    def test_duplicate_json_keys_are_rejected(self):
        with self.assertRaisesRegex(ValueError,'duplicate field'):
            c.fields('{"kind":"function","kind":"method"}')

if __name__ == '__main__':
    unittest.main(verbosity=2)

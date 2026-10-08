"""Hand-authored taxonomy witnesses, unrelated to admission examples."""
import inspect
import json
import unittest

from review import HERE, go_definitions, python_definitions, raw_array, raw_field

PYTHON = b'''# independent declaration-versus-runtime witness
def plain():
    return 0

class Vessel:
    def steer(self):
        def compute():
            return 1
        return compute()

    @staticmethod
    def calibrate():
        return 2

    @classmethod
    def construct(cls):
        return cls()

    @property
    def heading(self):
        return 3

    class Instrument:
        def read(self):
            return 4

def factory():
    class Local:
        def action(self):
            return 5
    def nested():
        return 6
    return Local, nested
'''

GO = '''package miniature
// Unicode padding: 船. func (f Fake) Ghost() {} is only a comment.
type Hull struct{}
type Meter struct{}
var text = "func (f Fake) Ghost() {}"
func Ordinary() int { return 0 }
func (h Hull) Move() int { return 1 }
func (h *Hull) Adjust() int { return 2 }
func (m Meter) Move() int { return 3 }
func Wrapper() func() int { return func() int { return 4 } }
'''.encode()

class TaxonomyTests(unittest.TestCase):
    def test_python_hand_declared_ownership(self):
        actual = {d['scope']:(d['kind'],d['subtype']) for d in python_definitions(PYTHON)}
        expected = {
            'plain':('function','function'), 'Vessel':('class','class'),
            'Vessel.steer':('method','instance_method'),
            'Vessel.steer.compute':('function','function'),
            'Vessel.calibrate':('method','static_method'),
            'Vessel.construct':('method','class_method'),
            'Vessel.heading':('method','property_accessor'),
            'Vessel.Instrument':('class','class'),
            'Vessel.Instrument.read':('method','instance_method'),
            'factory':('function','function'), 'factory.Local':('class','class'),
            'factory.Local.action':('method','instance_method'),
            'factory.nested':('function','function')}
        self.assertEqual(actual, expected)

    def test_python_runtime_binding_is_separate(self):
        namespace = {}
        exec(PYTHON, namespace)
        vessel = namespace['Vessel']
        self.assertTrue(inspect.isfunction(vessel.steer))
        self.assertTrue(inspect.ismethod(vessel().steer))
        self.assertTrue(inspect.isfunction(vessel.calibrate))
        self.assertIs(vessel.calibrate, vessel().calibrate)
        self.assertTrue(inspect.ismethod(vessel.construct))
        self.assertIs(vessel.construct.__self__, vessel)
        self.assertIsInstance(vessel.__dict__['heading'], property)
        _, nested = namespace['factory']()
        self.assertTrue(inspect.isfunction(nested))

    def test_go_receiver_declarations_hand_truth(self):
        definitions = go_definitions(GO)
        actual = [(d['name'],d['kind'],d['receiver']) for d in definitions]
        self.assertEqual(actual, [('Ordinary','function',''), ('Move','method','Hull'),
                                 ('Adjust','method','Hull'), ('Move','method','Meter'),
                                 ('Wrapper','function','')])
        self.assertEqual(sum(d['kind']=='method' for d in definitions),3)
        for d in definitions:
            self.assertTrue(GO[d['start']:d['end']].startswith(b'func '))

    def test_exact_raw_tokens_not_reserialized(self):
        text = '{ "query" : "a\\u0062", "answers": [ { "alternatives" : [ { "kind" : "function" } ] } ] }\n'
        self.assertEqual(raw_field(text,'query'), '"a\\u0062"')
        [(group, raw_group)] = raw_array(raw_field(text,'answers'))
        [(alt, raw_alt)] = raw_array(raw_field(raw_group,'alternatives'))
        self.assertEqual(raw_alt, '{ "kind" : "function" }')
        self.assertEqual(alt, {'kind':'function'})

    def test_all_items_remain_open_and_kind_only(self):
        rows = json.loads((HERE/'item-review.json').read_text())
        self.assertEqual(len(rows),166)
        self.assertTrue(all(not r['applied'] and r['protocol_resolution']=='open_versioned_proposal_required' for r in rows))
        self.assertTrue(all(r['original_kind']=='function' and r['declaration_kind']=='method' for r in rows))
        self.assertTrue(all(r['owner_reviews'] and r['sourcecoords']['start']==r['declarationcoords']['start'] for r in rows))
        proposal = json.loads((HERE/'versioned-proposal.json').read_text())
        self.assertEqual(proposal['confirmed_error_patch_items'],[])

if __name__ == '__main__': unittest.main(verbosity=2)

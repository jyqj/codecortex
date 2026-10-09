"""Bounded receipt corruption controls; no native or historical original replay."""
import copy
import unittest
from lifecycle_cargo_audit import RELEASE_PROFILE,verify_original_cargo

def fixture():
    source={'source_commit':'3ffcefc3b28ee1a4ed80caecebd7208a45c3e302','input_count':1092}
    producer='/home/runner/work/codecortex/codecortex';target='/home/runner/temp/p8-lifecycle/product/cargo-target'
    def event(package,name,src):
        return {'reason':'compiler-artifact','fresh':False,'features':[],'profile':dict(RELEASE_PROFILE),
                'manifest_path':f'{producer}/crates/{package}/Cargo.toml','package_id':f'path+file://{producer}/crates/{package}#0.1.0',
                'target':{'name':name,'kind':['bin'],'crate_types':['bin'],'src_path':f'{producer}/crates/{package}/{src}'},
                'executable':f'{target}/release/{name}'}
    a=event('cc-server','codecortex','src/main.rs');b=event('cc-eval','p8-measurements','src/bin/p8-measurements.rs')
    product={'package_kind':'default','build_profile':'release','cargo_target_initially_absent':True,'cargo_target_dir':target,'cargo_build_dir':target,
             'binary_path':'/home/runner/temp/p8-lifecycle/product/codecortex','actual_cargo_profile':dict(RELEASE_PROFILE),'cargo_artifact':a,
             'source_before':source,'source_after':source,'build_exit_code':0,
             'build_command':['cargo','build','-p','cc-server','--bin','codecortex','--no-default-features','--locked','--message-format=json-render-diagnostics','--offline','--release']}
    replay={'artifact':b,'source_before':source,'source_after':source,'build_exit_code':0,'binary_sha256':'a'*64,
            'copy_source':{'path':b['executable'],'sha256':'a'*64},
            'build_command':['cargo','build','--offline','--locked','--release','-p','cc-eval','--bin','p8-measurements','--target-dir',target,'--message-format=json-render-diagnostics']}
    return product,replay,[copy.deepcopy(a),{'reason':'build-finished','success':True}],[copy.deepcopy(b),{'reason':'build-finished','success':True}]

class LifecycleCargoControls(unittest.TestCase):
    def test_clean_original_shape_and_default_feature_variant(self):
        args=fixture();verify_original_cargo(*args)
        args[1]['artifact']['features']=['default'];args[3][0]['features']=['default'];verify_original_cargo(*args)
    def test_resealed_nonrelease_profile_rejected(self):
        args=fixture()
        for p in [args[0]['actual_cargo_profile'],args[0]['cargo_artifact']['profile'],args[1]['artifact']['profile'],args[2][0]['profile'],args[3][0]['profile']]:p['debug_assertions']=True
        with self.assertRaises(AssertionError):verify_original_cargo(*args)
    def test_other_evaluator_producer_even_with_matching_log_rejected(self):
        args=fixture()
        for artifact in [args[1]['artifact'],args[3][0]]:
            for key in ['manifest_path','package_id']:artifact[key]=artifact[key].replace('/codecortex/codecortex/','/foreign/source/')
            artifact['target']['src_path']=artifact['target']['src_path'].replace('/codecortex/codecortex/','/foreign/source/')
        with self.assertRaises(AssertionError):verify_original_cargo(*args)
    def test_changed_exact_selection_command_rejected(self):
        for index in [0,1]:
            args=fixture();args[index]['build_command'][args[index]['build_command'].index('--bin')+1]='other-program'
            with self.subTest(index=index),self.assertRaises(AssertionError):verify_original_cargo(*args)
    def test_duplicate_or_failed_cargo_completion_rejected(self):
        for index in [2,3]:
            args=fixture();args[index].insert(0,copy.deepcopy(args[index][0]))
            with self.subTest(index=index,kind='duplicate'),self.assertRaises(AssertionError):verify_original_cargo(*args)
            args=fixture();args[index][-1]['success']=False
            with self.subTest(index=index,kind='failed'),self.assertRaises(AssertionError):verify_original_cargo(*args)
    def test_reused_or_foreign_target_rejected(self):
        args=fixture();args[1]['artifact']['fresh']=True;args[3][0]['fresh']=True
        with self.assertRaises(AssertionError):verify_original_cargo(*args)
        args=fixture();args[1]['artifact']['executable']='/foreign/p8-measurements';args[3][0]['executable']='/foreign/p8-measurements';args[1]['copy_source']['path']='/foreign/p8-measurements'
        with self.assertRaises(AssertionError):verify_original_cargo(*args)

if __name__=='__main__':unittest.main(verbosity=2)

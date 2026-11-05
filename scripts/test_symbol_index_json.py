"""Independent navigation schema and original UTF-8 source-position validation."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[1]
EXECUTABLE=None


def invoke(*args,cwd=ROOT):
    return subprocess.run([str(EXECUTABLE),*args],cwd=cwd,capture_output=True,check=False)


class SymbolIndexJsonTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if EXECUTABLE is None:
            raise unittest.SkipTest('pass the built Scarp executable')

    def parse(self,mode,path,cwd=ROOT):
        r=invoke('inspect',mode,'--json',str(path),cwd=cwd)
        self.assertEqual(r.returncode,0,r.stderr.decode('utf-8'))
        self.assertEqual(r.stderr,b'')
        self.assertTrue(r.stdout.endswith(b'\n'))
        obj=json.loads(r.stdout)
        self.assertEqual(obj['schema_version'],1)
        self.assertEqual(obj['kind'],'scarp.'+mode)
        self.assertEqual((obj['span_encoding'],obj['column_encoding'],obj['position_base']),('utf-8-bytes','unicode-scalar',1))
        return obj

    def validate(self,obj,folder):
        sources=[(Path(folder)/f['path']).read_bytes() for f in obj['files']]
        self.assertEqual([f['id'] for f in obj['files']],list(range(len(sources))))
        def location(loc):
            data=sources[loc['source']]
            a,b=loc['span']
            self.assertTrue(0<=a<=b<=len(data))
            for offset,position in [(a,loc['start']),(b,loc['end'])]:
                prefix=data[:offset].decode('utf-8')
                self.assertEqual(position,dict(line=prefix.count('\n')+1,column=len(prefix.rsplit('\n',1)[-1])+1))
            return data[a:b].decode('utf-8')
        symbols=obj['symbols']
        self.assertEqual([d['id'] for d in symbols],list(range(len(symbols))))
        for d in symbols:
            self.assertEqual(set(d),{'id','name','kind','owner','public','mutable','target_module','location'})
            self.assertIsInstance(d['public'],bool)
            self.assertIsInstance(d['mutable'],bool)
            if d['owner'] is not None:
                self.assertTrue(0<=d['owner']<len(symbols))
            if d['target_module'] is not None:
                self.assertTrue(0<=d['target_module']<len(sources))
            if d['location'] is None:
                self.assertIn(d['kind'],['builtin','contract_result'])
            elif d['kind']=='module':
                self.assertEqual(location(d['location']),'')
            else:
                self.assertEqual(location(d['location']),d['name'])
        for r in obj.get('references',[]):
            self.assertEqual(set(r),{'target','kind','location'})
            self.assertTrue(0<=r['target']<len(symbols))
            d=symbols[r['target']]
            token=location(r['location'])
            if r['kind']=='import':
                self.assertEqual(d['kind'],'module')
                self.assertTrue(token.startswith('"') and token.endswith('"'))
            else:
                self.assertEqual(token,d['name'])
        order=[(r['location']['source'],*r['location']['span']) for r in obj.get('references',[])]
        self.assertEqual(order,sorted(order))

    def test_complete_example_matches_memory_loader_snapshots_after_path_normalization(self):
        for mode in ['symbols','references']:
            obj=self.parse(mode,'examples/symbol_navigation.srp')
            self.validate(obj,ROOT)
            for f in obj['files']:
                f['path']='symbol_navigation.srp' if f['id']==0 else 'modules/navigation_values.srp'
            for d in obj['symbols']:
                if d['kind']=='module':
                    d['name']=obj['files'][d['location']['source']]['path']
            expected=json.loads((ROOT/f'examples/symbol_navigation.{mode}.json').read_bytes())
            self.assertEqual(obj,expected)

    def test_views_share_ids_and_repeated_output_is_byte_stable(self):
        source='examples/symbol_navigation.srp'
        symbols=self.parse('symbols',source)
        refs=self.parse('references',source)
        self.assertEqual(symbols['symbols'],refs['symbols'])
        self.assertNotIn('references',symbols)
        self.assertEqual(refs,self.parse('references',source))
        a=invoke('inspect','references','--json',source)
        b=invoke('inspect','references','--json',source)
        self.assertEqual(a.stdout,b.stdout)

    def test_unicode_bom_crlf_qualified_names_and_named_labels_preserve_bytes(self):
        with tempfile.TemporaryDirectory(prefix='scarp-symbols-') as folder:
            root=Path(folder)
            (root/'main café.srp').write_bytes('\ufeff// é🌍\r\nimport "lib.srp" as l; print(l . f(n: l . x));'.encode('utf-8'))
            (root/'lib.srp').write_bytes('// 🌍\r\npub bind x=2; pub pure funcra f(n: integer)->integer{return n;}'.encode('utf-8'))
            obj=self.parse('references','main café.srp',cwd=folder)
            self.validate(obj,folder)
            self.assertEqual(sum(r['kind']=='namespace' for r in obj['references']),2)
            self.assertEqual(sum(r['kind']=='argument_label' for r in obj['references']),1)

    def test_static_indexing_never_evaluates_runtime_defaults_contracts_or_outputs(self):
        with tempfile.TemporaryDirectory(prefix='scarp-symbols-') as folder:
            Path(folder,'main.srp').write_text('pub bind secret="DO-NOT-SERIALIZE"; funcra f(n: integer=1/0)->integer needs false; {return n;} print("OUT"); eprint("ERR"); print(f()); while true {continue;}',encoding='utf-8')
            obj=self.parse('references','main.srp',cwd=folder)
            self.validate(obj,folder)
            payload=json.dumps(obj)
            for hidden in ['DO-NOT-SERIALIZE','1/0','OUT','ERR']:
                self.assertNotIn(hidden,payload)

    def test_invalid_graphs_fail_without_inventory_and_preserve_diagnostic_schema(self):
        with tempfile.TemporaryDirectory(prefix='scarp-symbols-') as folder:
            root=Path(folder)
            for source in ['bind x=;', 'print(unknown);', 'bind x=1; bind x=2;', 'import "missing.srp" as m;']:
                (root/'main.srp').write_text(source,encoding='utf-8')
                r=invoke('inspect','references','--json','--diagnostic-format=json','main.srp',cwd=folder)
                self.assertEqual(r.returncode,1)
                self.assertEqual(r.stdout,b'')
                error=json.loads(r.stderr)
                self.assertEqual(error['schema_version'],1)
                self.assertTrue(error['code'].startswith('E'))

    def test_import_cycles_and_private_symbols_preserve_error_behavior(self):
        with tempfile.TemporaryDirectory(prefix='scarp-symbols-') as folder:
            root=Path(folder)
            for entry,lib in [('import "lib.srp" as l; print(l.x);','bind x=1;'),('import "lib.srp" as l;','import "main.srp" as m;')]:
                (root/'main.srp').write_text(entry,encoding='utf-8')
                (root/'lib.srp').write_text(lib,encoding='utf-8')
                normal=invoke('inspect','modules','--json','--diagnostic-format=json','main.srp',cwd=folder)
                indexed=invoke('inspect','symbols','--json','--diagnostic-format=json','main.srp',cwd=folder)
                self.assertEqual((normal.returncode,normal.stdout,normal.stderr),(indexed.returncode,indexed.stdout,indexed.stderr))

    def test_project_directory_and_root_traversal_reuse_existing_trusted_loader(self):
        with tempfile.TemporaryDirectory(prefix='scarp-symbols-') as folder:
            root=Path(folder)
            (root/'src').mkdir()
            (root/'Scarp.project').write_text('scarp-project: 1\nname: Navigation\nentry: src/main.srp\n',encoding='utf-8')
            (root/'src/main.srp').write_text('bind x=1; print(x);',encoding='utf-8')
            self.validate(self.parse('references','.',cwd=folder),folder)
            (root/'escape.srp').write_text('pub bind secret=1;',encoding='utf-8')
            (root/'src/main.srp').write_text('import "../escape.srp" as x;',encoding='utf-8')
            r=invoke('inspect','references','--json','.',cwd=folder)
            self.assertEqual(r.returncode,1)
            self.assertEqual(r.stdout,b'')

    def test_reference_query_snapshot_does_not_include_deleted_imports(self):
        with tempfile.TemporaryDirectory(prefix='scarp-symbols-') as folder:
            root=Path(folder)
            (root/'main.srp').write_text('import "lib.srp" as l; print(l.x);',encoding='utf-8')
            (root/'lib.srp').write_text('pub bind x=1;',encoding='utf-8')
            self.parse('references','main.srp',cwd=folder)
            (root/'lib.srp').unlink()
            r=invoke('inspect','references','--json','main.srp',cwd=folder)
            self.assertEqual(r.returncode,1)
            self.assertEqual(r.stdout,b'')
            (root/'main.srp').write_text('bind y=2; print(y);',encoding='utf-8')
            obj=self.parse('references','main.srp',cwd=folder)
            self.assertEqual(len(obj['files']),1)
            self.assertNotIn('x',[d['name'] for d in obj['symbols']])


if __name__=='__main__':
    if len(sys.argv)!=2:
        raise SystemExit('usage: test_symbol_index_json.py <built-scarp-executable>')
    EXECUTABLE=Path(sys.argv.pop()).resolve()
    unittest.main()

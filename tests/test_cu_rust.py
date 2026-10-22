"""Black-box contract tests for the standalone Rust CLI/MCP binaries."""
import json
import os
import re
import shutil
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BIN = ROOT / 'cu-runtime/target/release'

class RustTools(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not (BIN / 'cu-mcp').exists():
            raise unittest.SkipTest('Build cu-runtime release binaries first')

    def test_protocol_recovery(self):
        requests = ['{', '[]', json.dumps({'id': 1, 'method': 'tools/call', 'params': []}),
                    json.dumps({'id': 2, 'method': 'tools/list'}),
                    json.dumps({'id': 3, 'method': 'ping'})]
        p = subprocess.run([BIN/'cu-mcp'], input='\n'.join(requests)+'\n', text=True, capture_output=True, timeout=3)
        self.assertEqual(p.returncode, 0, p.stderr)
        replies = [json.loads(line) for line in p.stdout.splitlines()]
        self.assertEqual([r['error']['code'] for r in replies[:3]], [-32700,-32600,-32602])
        self.assertIn('cu_select_file', {tool['name'] for tool in replies[3]['result']['tools']})
        self.assertEqual(replies[4]['result'], {})

    def test_release_version_is_consistent_across_cli_input_and_mcp(self):
        version = re.search(r'^version = "([^"]+)"', (ROOT/'cu-runtime/Cargo.toml').read_text(), re.M)[1]
        for executable, argument, prefix in [('cu','--version','cu'),('cu-input','-V','cu-input')]:
            result = subprocess.run([BIN/executable, argument], capture_output=True, text=True, timeout=3)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(result.stdout.strip(),f'{prefix} {version}')
        request={'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2024-11-05'}}
        result=subprocess.run([BIN/'cu-mcp'],input=json.dumps(request)+'\n',capture_output=True,text=True,timeout=3)
        self.assertEqual(json.loads(result.stdout)['result']['serverInfo']['version'],version)

    def test_default_pointer_backend_is_the_bundled_input_helper(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            shutil.copy2(BIN/'cu',root/'cu')
            log=root/'input.json'
            helper=root/'cu-input'
            helper.write_text('#!/usr/bin/env python3\nimport json,sys\nfrom pathlib import Path\nPath('+repr(str(log))+').write_text(json.dumps(sys.argv[1:]))\n')
            helper.chmod(0o755)
            env=dict(os.environ,CU_DIR=str(root/'state'))
            env.pop('CLICLICK',None)
            result=subprocess.run([root/'cu','click','10','20'],env=env,text=True,capture_output=True,timeout=3)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertEqual(json.loads(log.read_text()),['m:10,20','w:35','c:10,20'])

    def test_stdin_and_native_read_without_capture(self):
        with tempfile.TemporaryDirectory() as temp:
            helper = Path(temp)/'native'
            helper.write_text('#!/usr/bin/env python3\nimport json,sys\nprint(json.dumps({"ok":True,"args":sys.argv[1:],"input":sys.stdin.read()}))\n')
            helper.chmod(0o755)
            env = dict(os.environ, CU_BIN=str(BIN/'cu'), CU_NATIVE=str(helper), CU_SCREENCAPTURE='/not/available')
            request = {'id':1,'method':'tools/call','params':{'name':'cu_read','arguments':{'app':'Safari'}}}
            p = subprocess.run([BIN/'cu-mcp'], input=json.dumps(request)+'\n', text=True, capture_output=True, env=env, timeout=3)
            result = json.loads(p.stdout)['result']
            self.assertFalse(result['isError'])
            self.assertEqual(result['structuredContent']['args'], ['document-context','Safari'])
            env['CU_BIN'] = str(helper)
            request['params'] = {'name':'cu_type','arguments':{'text':'--private\nhello','secret':True}}
            p = subprocess.run([BIN/'cu-mcp'], input=json.dumps(request)+'\n', text=True, capture_output=True, env=env, timeout=3)
            result = json.loads(p.stdout)['result']['structuredContent']
            self.assertEqual(result['input'], '--private\nhello')
            self.assertNotIn('--private\nhello',result['args'])
            self.assertIn('--stdin',result['args'])

    def test_disabled_snapshot_cannot_activate_or_capture_or_click(self):
        import time
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            snapshots = root/'state'/'snapshots'
            snapshots.mkdir(parents=True)
            snapshot = 's_12_34'
            (snapshots/(snapshot+'.tsv')).write_text(
                f'META\t{snapshot}\t{int(time.time())}\tDemo\t4242\t42\t100\t200\t800\t600\tcom.example.Demo\tDemo Window\n'
                'EL\te_1\tnative_ax\tw0.0\tAXButton\tCheckout\t125\t235\t50\t30\t1.000\tAXPress\t\tfalse\n')
            log = root/'calls'
            helper = root/'native'
            helper.write_text('#!/bin/sh\nprintf called >> "'+str(log)+'"\nexit 1\n')
            helper.chmod(0o755)
            env = dict(os.environ, CU_DIR=str(root/'state'), CU_NATIVE=str(helper),
                       CU_SCREENCAPTURE=str(helper), CLICLICK=str(helper))
            for flags in ([], ['--native-only', '--background']):
                result = subprocess.run([BIN/'cu', 'act', 'e_1', '--snapshot', snapshot, '--json', *flags], env=env, capture_output=True, text=True, timeout=3)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('control_disabled', result.stdout)
                self.assertFalse(log.exists(), 'disabled target invoked a native/capture/input helper')

    def test_oversized_request_fails_bounded(self):
        p = subprocess.run([BIN/'cu-mcp'], input='x'*300000, text=True, capture_output=True, timeout=3)
        self.assertEqual(json.loads(p.stdout)['error']['code'],-32600)

if __name__ == '__main__':
    unittest.main()

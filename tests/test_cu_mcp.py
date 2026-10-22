"""Public MCP contract tests; no desktop mutations or model calls."""
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('cu_mcp', Path(__file__).resolve().parents[1] / 'tests/reference/cu-mcp.py')
mcp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mcp)

class MCPTests(unittest.TestCase):
    def test_compatible_default_and_explicit_native_only(self):
        with patch.object(mcp, 'invoke', return_value=(0, '{}')) as invoke:
            mcp.call_tool('cu_observe', {'app': 'Notes'})
            self.assertNotIn('--native-only', invoke.call_args.args[0])
            mcp.call_tool('cu_observe', {'app': 'Notes', 'native_only': True})
            self.assertIn('--native-only', invoke.call_args.args[0])

    def test_text_never_in_argv(self):
        with patch.object(mcp, 'invoke', return_value=(0, '{}')) as invoke:
            mcp.call_tool('cu_type', {'text': '--secret private text', 'secret': True})
            self.assertNotIn('--secret private text', invoke.call_args.args[0])
            self.assertIn('--stdin', invoke.call_args.args[0])
            self.assertEqual(invoke.call_args.args[1], '--secret private text')

    def test_invalid_arguments_never_execute(self):
        cases = [('cu_observe', {'app': 'Notes', 'max_elements': True}),
                 ('cu_observe', {'app': 'Notes', 'surprise': True}),
                 ('cu_wait', {'app': 'Notes', 'mode': 'shell'}),
                 ('cu_wait', {'app': 'Notes', 'mode': 'window', 'timeout_ms': -1}),
                 ('cu_click', {'x': float('nan'), 'y': 1}),
                 ('cu_click', {'x': 1, 'y': 1, 'window_relative': True}),
                 ('cu_type', {'text': 'x\x00'}), ('cu_type', []),
                 ('cu_type', {'text': 'x'*65537})]
        with patch.object(mcp, 'invoke') as invoke:
            for name, args in cases:
                with self.subTest(name=name, args_type=type(args)), self.assertRaises(ValueError):
                    mcp.call_tool(name, args)
            invoke.assert_not_called()

    def test_protocol_recovers_after_bad_requests(self):
        requests = ['{', '[]', json.dumps({'id': 1, 'method': 'tools/call', 'params': []}),
                    json.dumps({'id': 2, 'method': 'tools/list'})]
        out = io.StringIO()
        with patch('sys.stdin', io.StringIO('\n'.join(requests))), patch('sys.stdout', out):
            mcp.main()
        replies = [json.loads(line) for line in out.getvalue().splitlines()]
        self.assertEqual([r['error']['code'] for r in replies[:3]], [-32700, -32600, -32602])
        self.assertEqual(replies[3]['id'], 2)
        self.assertEqual(len(replies[3]['result']['tools']), 9)

    def test_missing_cli_is_structured(self):
        with patch.object(mcp, 'CU', '/nonexistent/cu-test'):
            code, output = mcp.invoke(['observe', 'Notes'])
        self.assertEqual(code, 127)
        self.assertEqual(json.loads(output)['error']['code'], 'cli_unavailable')

    def test_timeout_kills_group_without_retry(self):
        with patch.object(mcp.subprocess, 'Popen') as launch, patch.object(mcp.os, 'killpg') as kill:
            launch.return_value.pid = 123
            launch.return_value.wait.side_effect = [subprocess.TimeoutExpired('cu', 65), 0]
            code, output = mcp.invoke(['act', 'e_1'])
            self.assertEqual(code, 124)
            self.assertEqual(json.loads(output)['error']['code'], 'cli_timeout')
            launch.assert_called_once()
            kill.assert_called_once_with(123, mcp.signal.SIGKILL)

if __name__ == '__main__':
    unittest.main()

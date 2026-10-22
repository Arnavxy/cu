#!/usr/bin/env python3
"""Deterministic CLI/MCP overhead benchmark. Never accesses the desktop.

Extracts the exact baseline tag into a temporary directory. Both versions use
identical mock native/input/image helpers, including a real semantic state
transition. Results do not predict application, screen capture, OCR, or AI speed.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import random
import selectors
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]

MOCK_NATIVE = r'''#!/bin/zsh
print -r -- "${1:-missing}" >> "$CU_BENCH_CALLS"
state=0
[[ -f "$CU_BENCH_MARKER" ]] && read -r state < "$CU_BENCH_MARKER"
case "$1" in
context)
 printf 'META\t42\t4242\t100\t200\t800\t600\tDemo\tDemo Window\tcom.example.Demo\n'
 printf 'AX\tw0.0\tAXButton\tSave\t125\t235\t50\t30\tAXPress\t%s\n' "$state" ;;
ax-tree)
 printf 'w0.0\tAXButton\tSave\t125\t235\t50\t30\tAXPress\t%s\n' "$state" ;;
bounds|activate|windows) printf '42\t4242\t100\t200\t800\t600\tDemo\tDemo Window\n' ;;
app-info) printf '4242\tDemo\tcom.example.Demo\n' ;;
ax-status) print trusted ;;
ax-perform) print 1 > "$CU_BENCH_MARKER"; print AXPress ;;
fingerprint) print -r -- "state-$state" ;;
display) printf '1\t0\t0\t1440\t900\n' ;;
*) print -u2 "Unsupported mock native operation"; exit 64 ;;
esac
'''
MOCK_CAPTURE = '#!/bin/zsh\nprint -r -- capture >> "$CU_BENCH_CALLS"\nprint -n image > "${@[-1]}"\n'
MOCK_INPUT = '#!/bin/zsh\nprint -r -- input >> "$CU_BENCH_CALLS"\nexit 0\n'
MOCK_OSA = '#!/bin/zsh\nprint -r -- osascript >> "$CU_BENCH_CALLS"\nexit 0\n'
MOCK_SIPS = '''#!/bin/zsh
print -r -- sips >> "$CU_BENCH_CALLS"
output=""
for (( i=1; i <= $#; i++ )); do
 [[ "${@[$i]}" == --out ]] && output="${@[$((i+1))]}"
done
[[ -n "$output" ]] && print -n image > "$output"
exit 0
'''


def digest(data):
    return hashlib.sha256(data).hexdigest()


def run(command, env, stdin=None):
    result = subprocess.run(command, env=env, input=stdin, capture_output=True,
                            text=True, timeout=20)
    if result.returncode:
        raise RuntimeError(f"Command failed ({result.returncode}): {command[1:]}: {result.stdout[:500]} {result.stderr[:300]}")
    return result.stdout


def parsed_observation(raw):
    obj = json.loads(raw)
    assert obj.get('ok') is True and obj.get('source') == 'native_ax', obj
    elements = [e for e in obj['elements'] if e['name'] == 'Save']
    assert len(elements) == 1 and 'AXPress' in elements[0]['actions'], obj
    return obj['snapshot'], elements[0]['id']


def validate_action(raw, marker):
    obj = json.loads(raw)
    assert obj.get('ok') is True, obj
    assert marker.read_text().strip() == '1', 'Mock did not receive AXPress'
    verification = obj.get('verification', {})
    assert verification.get('changed') is True or verification.get('semantic_effect') == 'changed', obj
    return obj


class MCP:
    def __init__(self, command, env):
        self.process = subprocess.Popen(command, env=env, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                        text=True, bufsize=1)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.serial = 0
        self.reply_bytes = 0

    def call(self, method, params=None):
        self.serial += 1
        request = {'jsonrpc': '2.0', 'id': self.serial, 'method': method}
        if params is not None:
            request['params'] = params
        self.process.stdin.write(json.dumps(request, separators=(',', ':')) + '\n')
        self.process.stdin.flush()
        if not self.selector.select(20):
            raise RuntimeError('MCP response timed out')
        raw = self.process.stdout.readline()
        self.reply_bytes = len(raw.encode())
        result = json.loads(raw)
        assert result.get('id') == self.serial, result
        return result

    def tool(self, name, arguments):
        result = self.call('tools/call', {'name': name, 'arguments': arguments})
        assert 'error' not in result and not result['result'].get('isError'), result
        content = result['result']
        if 'structuredContent' in content:
            return json.dumps(content['structuredContent'])
        return content['content'][0]['text']

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.process.stdout.close()
        self.selector.close()


def summary(samples):
    ordered = sorted(samples)
    return {'samples': len(samples), 'median_ms': round(statistics.median(samples), 3),
            'p95_ms': round(ordered[max(0, math.ceil(len(ordered) * .95) - 1)], 3),
            'min_ms': round(ordered[0], 3), 'max_ms': round(ordered[-1], 3),
            'raw_ms': [round(x, 3) for x in samples]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--samples', type=int, default=30)
    parser.add_argument('--warmup', type=int, default=5)
    parser.add_argument('--baseline', default='v0.4.1')
    parser.add_argument('--rust-bin-dir', type=Path, default=ROOT/'cu-runtime/target/release')
    parser.add_argument('--output', type=Path, default=ROOT/'artifacts/rust-benchmark.json')
    args = parser.parse_args()
    if not 5 <= args.samples <= 1000 or not 0 <= args.warmup <= 100:
        parser.error('Use 5–1000 samples and 0–100 warmups')
    rust_dir = args.rust_bin_dir.resolve()
    for name in ('cu', 'cu-mcp'):
        if not (rust_dir/name).is_file():
            parser.error(f'Missing release binary: {rust_dir/name}; build Rust first')
    report = {'schema': 1, 'scope': 'Mocked process/protocol overhead; NOT real UI latency',
              'host': {'os': platform.platform(), 'machine': platform.machine(),
                       'python': sys.version.split()[0]},
              'samples': args.samples, 'warmup': args.warmup,
              'timer': 'time.perf_counter_ns; nearest-rank p95', 'baseline': args.baseline,
              'scenarios': {}, 'unsupported': {'legacy_mcp_ping': 'Published v0.4.1 has no ping method'}}
    with tempfile.TemporaryDirectory(prefix='cu-rust-benchmark-') as directory:
        root = Path(directory)
        legacy = root/'legacy'
        legacy.mkdir()
        source_hashes = {}
        for source, target in [('bin/cu', legacy/'cu'), ('scripts/cu-mcp.py', legacy/'cu-mcp.py')]:
            raw = subprocess.check_output(['git', 'show', f'{args.baseline}:{source}'], cwd=ROOT)
            target.write_bytes(raw)
            target.chmod(0o700)
            source_hashes[source] = digest(raw)
        report['baseline_source_sha256'] = source_hashes
        report['binaries'] = {name: {'bytes': (rust_dir/name).stat().st_size,
                                     'sha256': digest((rust_dir/name).read_bytes())}
                              for name in ('cu', 'cu-mcp', 'cu-native', 'cu-input') if (rust_dir/name).is_file()}
        report['python_executable'] = sys.executable
        helpers = {}
        for name, source in [('native', MOCK_NATIVE), ('capture', MOCK_CAPTURE),
                             ('input', MOCK_INPUT), ('osascript', MOCK_OSA), ('sips', MOCK_SIPS)]:
            helper = root/name
            helper.write_text(source)
            helper.chmod(0o700)
            helpers[name] = str(helper)
        report['mock_sources_sha256'] = {name: digest(Path(path).read_bytes()) for name, path in helpers.items()}
        environments = {}
        commands = {'legacy': [str(legacy/'cu')], 'rust': [str(rust_dir/'cu')]}
        mcp_commands = {'legacy': [sys.executable, str(legacy/'cu-mcp.py')], 'rust': [str(rust_dir/'cu-mcp')]}
        # Do not inherit helper overrides or mock fault-injection settings.
        clean = {k: v for k, v in os.environ.items() if not k.startswith(('CU_', 'CLICLICK'))}
        for name in commands:
            state = root/name
            state.mkdir(exist_ok=True)
            environments[name] = dict(clean, CU_DIR=str(state/'state'),
                CU_BIN=commands[name][0], CU_NATIVE=helpers['native'], CLICLICK=helpers['input'],
                CU_SCREENCAPTURE=helpers['capture'], CU_SIPS=helpers['sips'], CU_OSASCRIPT=helpers['osascript'],
                CU_VERIFY_DELAY='0', CU_AX_COORDINATE_FALLBACK='0', CU_CLICK_SETTLE_MS='0',
                CU_BENCH_MARKER=str(state/'marker'), CU_BENCH_CALLS=str(state/'calls'))
        sessions = {name: MCP(mcp_commands[name], environments[name]) for name in commands}
        try:
            for session in sessions.values():
                assert 'result' in session.call('initialize')
            def reset(name):
                env = environments[name]
                Path(env['CU_BENCH_MARKER']).write_text('0\n')
                Path(env['CU_BENCH_CALLS']).write_text('')
                shutil.rmtree(Path(env['CU_DIR'])/'snapshots', ignore_errors=True)

            def perform(name, scenario):
                env = environments[name]
                marker = Path(env['CU_BENCH_MARKER'])
                if scenario == 'cli_version_fresh_process':
                    assert run(commands[name]+['--version'], env).strip().startswith('cu ')
                elif scenario == 'cli_native_only_background_verified_action':
                    snapshot, element = parsed_observation(run(commands[name]+['observe', 'Demo', '--background', '--native-only', '--json'], env))
                    validate_action(run(commands[name]+['act', element, '--snapshot', snapshot, '--background', '--native-only', '--json'], env), marker)
                elif scenario in ('cli_observe', 'cli_observe_verified_action'):
                    snapshot, element = parsed_observation(run(commands[name]+['observe', 'Demo', '--json'], env))
                    if scenario.endswith('action'):
                        validate_action(run(commands[name]+['act', element, '--snapshot', snapshot, '--json'], env), marker)
                elif scenario == 'mcp_initialize_fresh_process':
                    session = MCP(mcp_commands[name], env)
                    try:
                        assert 'result' in session.call('initialize')
                    finally:
                        session.close()
                elif scenario == 'mcp_tools_list_persistent':
                    result = sessions[name].call('tools/list')
                    assert result['result']['tools']
                elif scenario == 'mcp_observe_verified_action_persistent':
                    snapshot, element = parsed_observation(sessions[name].tool('cu_observe', {'app': 'Demo'}))
                    validate_action(sessions[name].tool('cu_act', {'element': element, 'snapshot': snapshot}), marker)
                elif scenario == 'mcp_ping_persistent':
                    assert sessions[name].call('ping').get('result') == {}
                else:
                    raise AssertionError(scenario)

            scenarios = ['cli_version_fresh_process', 'cli_observe', 'cli_observe_verified_action',
                         'mcp_initialize_fresh_process', 'mcp_tools_list_persistent',
                         'mcp_observe_verified_action_persistent', 'mcp_ping_persistent',
                         'cli_native_only_background_verified_action']
            rng = random.Random(20261022)
            for scenario in scenarios:
                names = ['rust'] if scenario in ('mcp_ping_persistent', 'cli_native_only_background_verified_action') else ['legacy', 'rust']
                durations = {name: [] for name in names}
                backend_calls = {}
                for iteration in range(args.warmup + args.samples):
                    order = names.copy()
                    rng.shuffle(order)
                    for name in order:
                        reset(name)
                        started = time.perf_counter_ns()
                        perform(name, scenario)
                        elapsed = (time.perf_counter_ns()-started)/1_000_000
                        if iteration >= args.warmup:
                            durations[name].append(elapsed)
                        backend_calls[name] = Path(environments[name]['CU_BENCH_CALLS']).read_text().splitlines()
                results = {name: summary(values) for name, values in durations.items()}
                results['last_sample_backend_calls'] = backend_calls
                if scenario == 'cli_native_only_background_verified_action':
                    assert not any(call in ('capture', 'fingerprint', 'input') for call in backend_calls['rust'])
                    results['caveat'] = 'New Rust-only API; background native-only verified action is not equivalent to the foreground visual-verification comparison.'
                if scenario == 'mcp_tools_list_persistent':
                    results['response_bytes'] = {name: sessions[name].reply_bytes for name in names}
                    results['caveat'] = 'Tool catalog sizes differ; this is the actual tools/list operation, not equal-byte serialization.'
                report['scenarios'][scenario] = results
                print(scenario + ': ' + ', '.join(f'{name} median={results[name]["median_ms"]}ms p95={results[name]["p95_ms"]}ms' for name in names), flush=True)
        finally:
            for session in sessions.values():
                session.close()
    for name, metadata in report['binaries'].items():
        if digest((rust_dir/name).read_bytes()) != metadata['sha256']:
            raise RuntimeError(f'Release binary changed during measurement: {name}; rerun after builds finish')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    print(f'Report: {args.output}')


if __name__ == '__main__':
    main()

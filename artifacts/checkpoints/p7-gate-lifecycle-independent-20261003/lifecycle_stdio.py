#!/usr/bin/env python3
"""Independent actual-product lifecycle; only synthetic loopback and stdio."""
import hashlib, http.server, json, os, pathlib, queue, subprocess, sys
import tempfile, threading, time, traceback

BINARY = pathlib.Path(sys.argv[1]).resolve()
OUTPUT = pathlib.Path(sys.argv[2]).resolve()
OUTPUT.mkdir(parents=True)
SOURCE = 'pub fn needle() -> u32 { 947 }\n'
MARKER = 'lifecycle_query_marker'
records, http_records, cases, children = [], [], [], []
lock = threading.Lock()
gate, entered = threading.Event(), threading.Event()
gate.set()
mode = {'status': 200}


def record(kind, **data):
    with lock:
        records.append({'sequence': len(records), 'kind': kind, **data})


def costs():
    with lock:
        rows = list(http_records)
    return {key: sum(r[key] for r in rows) for key in ('document_posts', 'query_posts', 'document_inputs', 'query_inputs', 'synthetic_reported_tokens')}


def delta(before):
    return {k: v - before[k] for k, v in costs().items()}


class Endpoint(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_POST(self):
        assert self.client_address[0] == '127.0.0.1'
        assert self.path == '/v1/embeddings'
        assert self.headers.get('Authorization') == 'Bearer synthetic-lifecycle-public'
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        assert body['model'] == 'fake/independent-lifecycle'
        inputs = body['input']
        assert inputs and all(isinstance(t, str) for t in inputs)
        query = all(t.startswith('lifecycle_query_') for t in inputs)
        assert query or all('needle' in t and '947' in t for t in inputs), inputs
        entry = {'sequence': len(http_records), 'body': body, 'only_loopback': True,
                 'dummy_authorization_matched': True, 'document_posts': int(not query),
                 'query_posts': int(query), 'document_inputs': len(inputs) if not query else 0,
                 'query_inputs': len(inputs) if query else 0, 'response_status': mode['status'],
                 'synthetic_reported_tokens': len(inputs) if mode['status'] == 200 else 0}
        with lock:
            http_records.append(entry)
        entered.set()
        assert gate.wait(10), 'bounded fixture release required'
        payload = json.dumps({'model': body['model'], 'data': [{'index': n, 'embedding': [1.0, 0.0]} for n in range(len(inputs))], 'usage': {'prompt_tokens': len(inputs), 'total_tokens': len(inputs)}}).encode()
        self.send_response(entry['response_status'])
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        try:
            self.wfile.write(payload)
        except BrokenPipeError:
            record('fixture_disconnected', http_sequence=entry['sequence'])


server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Endpoint)
threading.Thread(target=server.serve_forever, daemon=True).start()
endpoint = f'http://127.0.0.1:{server.server_port}/v1'


def config(**overrides):
    semantic = {'enabled': True, 'network_opt_in': True, 'allow_query_network': True,
                'model_id': 'fake/independent-lifecycle', 'dimensions': 2,
                'max_input_tokens': 8192, 'max_batch_items': 16, 'endpoint': endpoint,
                'allow_http': True, 'api_key_ref': 'env:P7_LIFECYCLE_DUMMY',
                'breaker_failure_threshold': 100}
    semantic.update(overrides)
    return {'auto_index': {'enabled': False}, 'query': {'strategy': 'auto'}, 'semantic': semantic}


class Child:
    def __init__(self, root, cfg, label):
        self.root, self.label = root, label
        root.mkdir(exist_ok=True)
        (root / 'one.rs').write_text(SOURCE)
        (root / '.codecortex.json').write_text(json.dumps(cfg))
        env = {k: v for k, v in os.environ.items() if not k.startswith('CODECORTEX_') and k not in ('OPENAI_API_KEY', 'P7_LIFECYCLE_ABSENT')}
        env.update(CODECORTEX_SEMANTIC_CACHE_ROOT=str(root.parent / (root.name + '-cache')), CODECORTEX_PPID_POLL_MS='0', P7_LIFECYCLE_DUMMY='synthetic-lifecycle-public')
        self.cache = pathlib.Path(env['CODECORTEX_SEMANTIC_CACHE_ROOT'])
        self.log_path = OUTPUT / (label + '-stderr.log')
        self.log = self.log_path.open('wb')
        self.process = subprocess.Popen([str(BINARY), 'mcp', '--project-path', str(root)], cwd=root, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log, text=True)
        children.append(self)
        self.responses = queue.Queue()
        def read():
            for line in self.process.stdout:
                self.responses.put(json.loads(line))
        threading.Thread(target=read, daemon=True).start()
        self.n = 0
        self.rpc('initialize', {'protocolVersion': '2024-11-05', 'capabilities': {}, 'clientInfo': {'name': 'independent-lifecycle', 'version': '1'}})
        self.process.stdin.write(json.dumps({'jsonrpc': '2.0', 'method': 'notifications/initialized'}) + '\n')
        self.process.stdin.flush()
        assert len(self.rpc('tools/list', {})['tools']) == 14

    def rpc(self, method, params, error=False):
        self.n += 1
        request = {'jsonrpc': '2.0', 'id': self.n, 'method': method, 'params': params}
        self.process.stdin.write(json.dumps(request) + '\n')
        self.process.stdin.flush()
        limit = time.monotonic() + 10
        while time.monotonic() < limit:
            reply = self.responses.get(timeout=max(.01, limit-time.monotonic()))
            record('rpc', session=self.label, request=request, response=reply)
            if reply.get('id') == self.n:
                assert ('error' in reply) == error, reply
                return reply['error'] if error else reply['result']
        raise TimeoutError(method)

    def tool(self, name, args, error=False):
        reply = self.rpc('tools/call', {'name': name, 'arguments': args}, error)
        if error:
            return reply
        assert not reply.get('isError'), reply
        result = reply['structuredContent']
        return result.get('result', result)

    def status(self):
        return self.tool('status', {'aspect': 'capabilities'})['retrieval']

    def index(self):
        return self.tool('index', {'path': str(self.root)})

    def state(self, expected):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            status = self.status()
            if status['semantic_state'] == expected and status['query_pins'] == 0:
                return status
            time.sleep(.025)
        raise AssertionError((expected, status))

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            self.process.wait(timeout=10)
            self.log.close()
            assert self.process.returncode == 0
            record('close', session=self.label, exit_code=self.process.returncode, stderr_sha256=hashlib.sha256(self.log_path.read_bytes()).hexdigest())


def lane(value):
    rows = value['evidence_summary']['retrieval']
    return next(r for r in rows.get('lanes', rows.get('lane_receipts', [])) if r['lane_id'] == 'semantic')


def source(value):
    hits = value['machine_pack']['hits']
    assert hits and {h['file_path'] for h in hits} == {'one.rs'}, value
    for hit in hits:
        assert hit['text'] and hit['text'] in SOURCE
    assert value['spans'] and {s['file_path'] for s in value['spans']} == {'one.rs'}


def queries(child, ready=False, local=False):
    for name, key in [('search', 'query'), ('context', 'task')]:
        args = {key: 'needle' if local or not ready else MARKER + '_' + name, 'retrieval_strategy': 'local' if local else 'auto'}
        value = child.tool(name, args)
        source(value)
        if ready and not local:
            semantic = lane(value)
            assert semantic['status'] == 'complete' and semantic['candidate_count'] > 0
            assert any(term[0] == 'rrf:semantic' for h in value['machine_pack']['hits'] for term in h['score_trace'])
    return child.status()


def checkpoint(label, child, before, **assertions):
    row = {'case': label, 'session': child.label, 'status': child.status(), 'delta': delta(before), **assertions}
    cases.append(row)
    record('checkpoint', **row)


try:
    with tempfile.TemporaryDirectory(prefix='p7-lifecycle-review-') as tmp:
        base = pathlib.Path(tmp)
        for label, cfg, expected, reason in [
            ('unconfigured', {'auto_index': {'enabled': False}}, 'not_configured', 'semantic_disabled'),
            ('disabled', config(enabled=False), 'not_configured', 'semantic_disabled'),
            ('authority_absent', config(network_opt_in=False), 'port_attached_unverified', 'network_opt_in_required')]:
            before = costs()
            child = Child(base / label, cfg, label)
            child.index()
            status = queries(child, local=True)
            queries(child)
            assert status['semantic_state'] == expected and status['dense_state'] == 'disabled'
            assert status['query_encoding']['reason'] == reason
            assert delta(before)['document_posts'] == delta(before)['query_posts'] == 0
            assert not child.cache.exists()
            if expected == 'not_configured':
                for name, key in [('search', 'query'), ('context', 'task')]:
                    err = child.tool(name, {key: 'needle', 'retrieval_strategy': 'semantic'}, error=True)
                    assert err['code'] == -32603 and err['message'] == 'semantic recall is not configured'
            checkpoint(label, child, before, cache_created=False)
            child.close()
        before = costs()
        child = Child(base / 'assembly', config(api_key_ref='env:P7_LIFECYCLE_ABSENT'), 'assembly_failed')
        child.index()
        status = child.state('failed')
        assert status['semantic_worker_reason'] == 'semantic_provider_assembly_failed'
        assert status['dense_state'] != 'ready'
        queries(child, local=True)
        assert delta(before)['document_posts'] == delta(before)['query_posts'] == 0
        checkpoint('assembly_failed', child, before)
        child.close()

        root = base / 'durable'
        before = costs()
        gate.clear(); entered.clear()
        child = Child(root, config(), 'wired')
        assert child.status()['semantic_state'] == 'port_attached_unverified'
        child.index()
        assert entered.wait(5)
        pending = child.status()
        assert pending['semantic_state'] == 'backfilling' and pending['semantic_pending'] > 0
        assert pending['dense_state'] == 'partial' and pending['query_pins'] == 1
        checkpoint('backfilling', child, before, held_document_http=True)
        gate.set()
        ready = child.state('ready')
        assert ready['dense_state'] == 'ready' and ready['dense_desired'] == ready['dense_published'] == 1
        assert delta(before)['document_posts'] == delta(before)['document_inputs'] == 1
        queries(child, ready=True)
        queries(child, ready=True)
        assert delta(before)['query_posts'] == delta(before)['query_inputs'] == 2
        checkpoint('ready_cold_then_warm', child, before)
        child.close()

        before = costs()
        child = Child(root, config(), 'restart_same_project')
        status = child.state('ready')
        assert status['dense_published'] == 1
        child.index()
        queries(child, ready=True)
        queries(child, ready=True)
        assert delta(before)['document_posts'] == 0
        assert delta(before)['query_posts'] == 2, 'query vectors are in-process; restart is cold once per distinct tool marker'
        checkpoint('clean_restart_document_reuse_query_cold_then_warm', child, before)
        child.close()

        for label, changes, expected_reason in [
            ('query_authority_revoked', {'allow_query_network': False}, 'query_network_opt_in_required'),
            ('network_authority_revoked', {'network_opt_in': False}, 'network_opt_in_required'),
            ('disabled_after_ready', {'enabled': False}, 'semantic_disabled')]:
            before = costs()
            child = Child(root, config(**changes), label)
            queries(child)
            status = child.status()
            assert not status['query_encoding']['network_authorized']
            assert status['query_encoding']['reason'] == expected_reason
            assert delta(before)['document_posts'] == delta(before)['query_posts'] == 0
            if label == 'query_authority_revoked':
                assert status['semantic_state'] == 'ready'
                for name, key in [('search', 'query'), ('context', 'task')]:
                    value = child.tool(name, {key: MARKER + '_revoked_' + name, 'retrieval_strategy': 'semantic'})
                    assert value['machine_pack']['hits'] == [] and value['spans'] == [], 'nonlexical cold revoked query cannot invent local results'
                    assert lane(value)['status'] == 'unavailable' and lane(value)['candidate_count'] == 0
            checkpoint(label, child, before, reload='new process; no hot reload claim')
            child.close()

        before = costs()
        mode['status'] = 500
        child = Child(base / 'provider_fail', config(allow_query_network=False), 'provider_failure')
        for attempt in range(3):
            child.index()
            deadline = time.monotonic() + 10
            while costs()['document_posts'] - before['document_posts'] < attempt + 1 and time.monotonic() < deadline:
                time.sleep(.025)
            assert delta(before)['document_posts'] == attempt + 1
            if attempt < 2:
                status = child.state('backfilling')
                assert status['semantic_pending'] > 0 and status['dense_published'] == 0
                checkpoint('provider_error_retry_pending_' + str(attempt + 1), child, before)
                time.sleep(31)  # observe unchanged production 30s durable backoff, no queue edits
        status = child.state('failed')
        assert status['semantic_failed'] == 1 and status['dense_published'] == 0
        queries(child)
        assert delta(before)['document_posts'] == 3 and delta(before)['query_posts'] == 0
        checkpoint('terminal_provider_http500', child, before, production_attempt_budget=3, manual_queue_mutation=False)
        child.close()
    summary = {'scope': 'L3 actual product stdio; synthetic loopback', 'binary_sha256': hashlib.sha256(BINARY.read_bytes()).hexdigest(), 'cases': cases, 'costs': costs(), 'passed': True, 'paid_cost': None, 'synthetic_reported_tokens_are_not_real_cost': True, 'SIGKILL_matrix': 'not_run_P7_016', 'hot_reload': 'not_run'}
except BaseException as error:
    summary = {'passed': False, 'error': repr(error), 'traceback': traceback.format_exc(), 'cases': cases, 'costs': costs()}
    raise
finally:
    gate.set()
    for child in children:
        try:
            child.close()
        except Exception as error:
            record('cleanup_error', error=repr(error))
    server.shutdown()
    for filename, value in [('summary.json', summary), ('stdio-raw.json', records), ('http-raw.json', http_records)]:
        (OUTPUT / filename).write_text(json.dumps(value, indent=2) + '\n')

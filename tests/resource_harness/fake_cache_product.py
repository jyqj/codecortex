"""Self-written 32-file fixture, toy SQLite schema and two owned HTTP requests.

This cannot execute a 100k workload, and imports no codecortex code.
"""
import json
import os
from pathlib import Path
import sqlite3
import sys
import threading
import urllib.request

root = Path(sys.argv[1])
cache = Path(os.environ['CODECORTEX_SEMANTIC_CACHE_ROOT'])
config = json.loads((root / '.codecortex.json').read_text())
files = sorted((root / 'src').glob('*.rs'))
assert len(files) == 32, 'fixture restricted to exactly 32 harmless source strings'
db = root / '.codecortex/index.sqlite3'
done = threading.Event()
errors = []
initialized = False
if (cache / 'complete').exists():
    done.set()


def fill_cache():
    def request(batch):
        try:
            payload = json.dumps(dict(model='fake/resource-preparation', dimensions=128,
                                      input=[p.read_text() for p in batch])).encode()
            req = urllib.request.Request(config['semantic']['endpoint'] + '/embeddings', payload,
                headers={'Authorization': 'Bearer synthetic-resource-only', 'Content-Type': 'application/json'})
            with urllib.request.urlopen(req, timeout=5) as response:
                data = json.load(response)
                assert len(data['data']) == 16
        except BaseException as exc:
            errors.append(repr(exc))
    workers = [threading.Thread(target=request,args=(files[i:i+16],)) for i in (0,16)]
    for worker in workers:worker.start()
    for worker in workers:worker.join()
    if not errors:
        cache.mkdir(exist_ok=True)
        (cache / 'complete').write_text('toy fixture cache complete, 32 strings')
        done.set()


for line in sys.stdin:
    request = json.loads(line)
    method = request['method']
    if method == 'notifications/initialized':
        initialized = True
        continue
    if method == 'initialize':
        result = dict(protocolVersion='2024-11-05')
    elif method == 'tools/list':
        assert initialized
        result = dict(tools=[dict(name=f'fixture-{i}') for i in range(14)])
    else:
        name = request['params']['name']
        if name == 'index':
            db.parent.mkdir()
            connection = sqlite3.connect(db)
            for table in ('files','symbols','chunks','document_manifest','semantic_manifest'):
                connection.execute(f'CREATE TABLE {table}(id INTEGER PRIMARY KEY)')
                connection.executemany(f'INSERT INTO {table} VALUES (?)',[(i,) for i in range(32)])
            for table in ('co_change_edges','test_edges'):
                connection.execute(f'CREATE TABLE {table}(id INTEGER)')
            connection.execute('CREATE TABLE semantic_outbox(state TEXT, attempt_count INTEGER)')
            connection.executemany('INSERT INTO semantic_outbox VALUES (?,?)',[('done',1)]*32)
            connection.commit();connection.close()
            threading.Thread(target=fill_cache).start()
            data = dict(files_scanned=32,files_parsed=32,files_added=32,files_skipped=0,parse_errors=[])
        elif name == 'status':
            assert not errors, errors
            data = dict(retrieval=dict(spec='retrieval-capabilities-v2',consistency='point_in_time',
                generation_scope='observed_database_snapshot',service_state_scope='process_observed_separately',
                identity_validation='checked_at_observation_boundary',generation={'fixture':1},
                semantic_active_space='toy-space',semantic_state='ready' if done.is_set() else 'backfilling',
                index_state='available',error=None))
        else:
            query = request['params']['arguments']['query']
            data = dict(machine_pack=dict(hits=[dict(text=query)]))
        result = dict(structuredContent=dict(result=data))
    print(json.dumps(dict(jsonrpc='2.0',id=request['id'],result=result)),flush=True)

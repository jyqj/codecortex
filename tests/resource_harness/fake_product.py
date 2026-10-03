"""Self-written harmless stdio fixture; never imports production code."""
import json
import sys

initialized = False
for line in sys.stdin:
    request = json.loads(line)
    method = request['method']
    if method == 'notifications/initialized':
        initialized = True
        continue
    if method == 'initialize':
        assert request['params'] == dict(protocolVersion='2024-11-05', capabilities={},
            clientInfo=dict(name='resource-preparation', version='1'))
        result = dict(protocolVersion='2024-11-05')
    elif method == 'tools/list':
        assert initialized
        result = dict(tools=[dict(name=f'fixture-{i}') for i in range(14)])
    elif method == 'tools/call':
        args = request['params']
        if args['name'] == 'wrapped':
            result = dict(structuredContent=dict(result=args['arguments']))
        elif args['name'] == 'error':
            result = dict(isError=True, structuredContent=dict(result='fixture-error'))
        elif args['name'] == 'pending':
            # No response: tests release this owned process with ordinary EOF.
            continue
        else:
            result = dict(structuredContent=args['arguments'])
    else:
        result = request['params']
    print(json.dumps(dict(jsonrpc='2.0', id=request['id'], result=result)), flush=True)

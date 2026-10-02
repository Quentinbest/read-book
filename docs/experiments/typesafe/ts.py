import json, os, time, urllib.request, urllib.error

URL = 'https://api.typesafe.ai/v1/systemone'
STATS = {'calls': 0, 'in': 0, 'out': 0, 'ms': []}


def ask(state, questions, model='jev-latest', tries=6):
    body = json.dumps({'state': state, 'model': model, 'questions': questions}).encode()
    for attempt in range(tries):
        req = urllib.request.Request(URL, data=body, method='POST', headers={
            'Authorization': 'Bearer ' + os.environ['TYPESAFE_API_KEY'],
            'Content-Type': 'application/json',
        })
        t0 = time.time()
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                res = json.load(r)
            STATS['calls'] += 1
            STATS['ms'].append((time.time() - t0) * 1000)
            STATS['in'] += res['usage']['input_tokens']
            STATS['out'] += res['usage']['output_tokens']
            return res
        except urllib.error.HTTPError as e:
            if e.code in (429, 529, 500, 502, 503) and attempt < tries - 1:
                time.sleep(2 ** attempt)
                continue
            raise RuntimeError(f'{e.code}: {e.read().decode()[:500]}')


def report():
    ms = sorted(STATS['ms']) or [0]
    return (f"calls={STATS['calls']} tokens_in={STATS['in']} out={STATS['out']} "
            f"p50={ms[len(ms)//2]:.0f}ms p90={ms[int(len(ms)*.9)]:.0f}ms")

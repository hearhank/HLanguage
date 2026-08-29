import sys

lines = []
with open(sys.argv[1], encoding='utf-8', errors='replace') as f:
    for raw in f:
        stripped = raw.lstrip(' ')
        d = len(raw) - len(stripped) - 1
        lines.append((d, stripped.rstrip('\n')))

def walk(target):
    chain = []
    d, s = lines[target]
    chain.append((target, d, s))
    for i in range(target - 1, -1, -1):
        d2, s2 = lines[i]
        if d2 < 0:
            continue
        if d2 < d:
            chain.append((i, d2, s2))
            d = d2
            if d <= 1:
                break
    return chain

for i, d, s in reversed(walk(int(sys.argv[2]))[-12:]):
    print(f'L{i+1} d={d} {s[:80]}')

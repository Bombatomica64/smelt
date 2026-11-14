import subprocess,re,collections
ROOT = __import__("subprocess").check_output(["git", "rev-parse", "--show-toplevel"], cwd=__import__("os").path.dirname(__import__("os").path.abspath(__file__))).decode().strip()
def sites():
    files=subprocess.check_output(['git','ls-files','crates/smelt-frontend-ts/src','crates/smelt-frontend-py/src','crates/smelt-codegen-rust/src'],cwd=ROOT).decode().split()
    by=collections.defaultdict(collections.Counter)
    for f in files:
        if not f.endswith('.rs') or '/tests/' in f or f.endswith('_tests.rs'): continue
        t=open(f'{ROOT}/{f}').read().split('#[cfg(test)]')[0]
        for line in t.splitlines():
            s=line.strip()
            if s.startswith('//'): continue
            for m in re.finditer(r'ErasureReason::(\w+)',s):
                parts=f.split('/')
                crate=parts[1]
                rest=parts[3:]
                mod='/'.join(rest[:2]).replace('.rs','') if rest[0] in ('lowering','emitter') and len(rest)>1 else rest[0].replace('.rs','')
                by[m.group(1)][(crate,mod)]+=1
    return by
if __name__=='__main__':
    for r,c in sorted(sites().items(),key=lambda x:-sum(x[1].values())): print(r,sum(c.values()),c.most_common(3))

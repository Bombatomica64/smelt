import subprocess,collections,os,sys,json
out=subprocess.check_output(['git','log','--since=60 days ago','--no-merges','--name-only','--format=@@%h']).decode()
DEEP={'crates/smelt-frontend-ts/src/lowering','crates/smelt-codegen-rust/src/emitter','crates/smelt-runtime/src/runtime','crates/smelt-frontend-py/src/lowering','crates/smelt-mir/src/lower','crates/smelt-mir/src/opt'}
def key(f):
    if not f.startswith('crates/') or not f.endswith('.rs'): return None
    p=f.split('/')
    if len(p)<4 or p[2]!='src': return '/'.join(p[:3])  # crate/tests etc
    base='/'.join(p[:4]); 
    if base.endswith('.rs'): base=base[:-3]
    if base in DEEP and len(p)>4:
        s=p[4]; s=s[:-3] if s.endswith('.rs') else s
        return base+'/'+s
    return base
commits=collections.defaultdict(set); filecommits=collections.Counter()
cur=None
for line in out.splitlines():
    if line.startswith('@@'): cur=line[2:]; continue
    if not line.strip(): continue
    filecommits[line]+=1
    k=key(line)
    if k: commits[k].add(cur)
# sizes
files=[f for f in subprocess.check_output(['git','ls-files','crates']).decode().split() if f.endswith('.rs')]
size=collections.Counter()
for f in files:
    k=key(f)
    if k: size[k]+=sum(1 for _ in open(f,errors='ignore'))
rows=sorted(((len(commits[k]),size[k],k) for k in set(size)|set(commits)),reverse=True)
json.dump([{'module':k,'commits':c,'lines':s} for c,s,k in rows],open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'churn.json'),'w'),indent=0)
for c,s,k in rows[:70]: print(f'{c:5d} {s:7d} {k}')
print('--- top files')
for f,c in filecommits.most_common(40): print(c,f)
print('--- numstat')
out=subprocess.check_output(['git','log','--since=60 days ago','--no-merges','--numstat','--format=@@%h']).decode()
ch=collections.Counter()
for line in out.splitlines():
    p=line.split('\t')
    if len(p)==3 and p[0]!='-':
        f=p[2]
        if '=>' in f: continue
        k=key(f)
        if k: ch[k]+=int(p[0])+int(p[1])
d=json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'churn.json')))
for r in d: r['changed']=ch[r['module']]
json.dump(d,open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'churn.json'),'w'),indent=0)
for k,v in ch.most_common(30): print(v,size[k],len(commits[k]),k)

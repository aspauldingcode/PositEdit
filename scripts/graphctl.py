#!/usr/bin/env python3
"""Canonical seed graph editor; does not permit changes to accepted laws."""
import argparse, importlib.util, json, pathlib, re, sys
ROOT=pathlib.Path(__file__).resolve().parents[1]
SEED=ROOT/'engineering/graph/seed.json'
spec=importlib.util.spec_from_file_location('axiom_harness',ROOT/'scripts/harness.py')
h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)

def validate(data):
    ids=[n['id'] for n in data['nodes']]
    if len(ids)!=len(set(ids)):raise ValueError('Duplicate node ID')
    dependency={k:set() for k in ids}
    for edge in data['edges']:
        if edge['source'] not in dependency or edge['target'] not in dependency:raise ValueError('Dangling edge')
        if edge['type']=='DEPENDS_ON':dependency[edge['source']].add(edge['target'])
    remaining=set(ids)
    while remaining:
        ready={k for k in remaining if not (dependency[k] & remaining)}
        if not ready:raise ValueError('Cycle in DEPENDS_ON graph')
        remaining-=ready

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    subs=parser.add_subparsers(dest='action',required=True)
    add=subs.add_parser('add');add.add_argument('--id',required=True);add.add_argument('--title',required=True)
    add.add_argument('--type',choices=['task','defect','risk','test','benchmark','decision','artifact'],default='task')
    add.add_argument('--depends',nargs='*',default=[]);add.add_argument('--priority',type=int,default=60)
    add.add_argument('--scope',nargs='*',default=[])
    link=subs.add_parser('link');link.add_argument('source');link.add_argument('relation',choices=['DEPENDS_ON','IMPLEMENTS','VERIFIED_BY','PRODUCES','BLOCKS','AFFECTS','REGRESSES','SUPERSEDES','CONSTRAINED_BY']);link.add_argument('target')
    reopen=subs.add_parser('reopen');reopen.add_argument('id');reopen.add_argument('--downstream',action='store_true')
    args=parser.parse_args()
    data=json.loads(SEED.read_text())
    ids={n['id'] for n in data['nodes']}
    if args.action=='add':
        if args.id in ids:sys.exit('Duplicate ID')
        if not re.fullmatch('[A-Za-z][A-Za-z0-9_-]{2,63}',args.id):sys.exit('Invalid node ID')
        if not all(x in ids for x in args.depends):sys.exit('Missing dependency')
        data['nodes'].append({'id':args.id,'type':args.type,'title':args.title,'priority':args.priority,'phase':6,
                              'status':'pending' if args.type=='task' else 'open','risk':'medium',
                              'acceptance_commands':['python3 scripts/verify.py --level scaffold'] if args.type=='task' else [],
                              'file_scope':args.scope,'max_attempts':3,'requires_proof':False,
                              'description':'Graph-added task. Author must supply nontrivial acceptance criteria before completion.'})
        for dep in args.depends:data['edges'].append({'source':args.id,'target':dep,'type':'DEPENDS_ON'})
    elif args.action=='link':
        if args.source not in ids or args.target not in ids:sys.exit('Unknown node ID')
        edge={'source':args.source,'target':args.target,'type':args.relation}
        if edge not in data['edges']:data['edges'].append(edge)
    elif args.action=='reopen':
        if args.id not in ids:sys.exit('Unknown node ID')
        deps={n['id']:set() for n in data['nodes']}
        for e in data['edges']:
            if e['type']=='DEPENDS_ON':deps[e['source']].add(e['target'])
        touched={args.id}
        if args.downstream:
            changed=True
            while changed:
                newer={n for n,d in deps.items() if d.intersection(touched)}
                changed=bool(newer-touched);touched|=newer
        # Accepted laws may never be downgraded.
        for node in data['nodes']:
            if node['id'] in touched and node['type']!='law':node['status']='pending'
        # Also update active runtime statuses and attempts below.
    validate(data)
    temp=SEED.with_suffix('.json.tmp');temp.write_text(json.dumps(data,indent=2)+'\n');temp.replace(SEED)
    h.SEED=data
    with h.connection() as conn:
        h.init(conn)
        if args.action=='reopen':
            for node in data['nodes']:
                if node['id'] in touched and node['type']=='task':
                    conn.execute("UPDATE nodes SET status='pending',attempts=0,updated=? WHERE id=?",(h.now(),node['id']))
            conn.commit()
    print('Graph seed updated:',args.action)
if __name__=='__main__':main()

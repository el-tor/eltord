#!/usr/bin/env python3
"""Summarize a completed soak without copying raw logs or host paths."""
import argparse, collections, json, math, pathlib, statistics

parser=argparse.ArgumentParser()
parser.add_argument('run',type=pathlib.Path)
args=parser.parse_args()
rows=[json.loads(line) for line in (args.run/'requests.jsonl').read_text().splitlines()]
origin={json.loads(line)['id'] for line in (args.run/'origin.jsonl').read_text().splitlines()}
groups=collections.defaultdict(list)
for row in rows: groups[row['phase']+'/'+row['route']].append(row)
report={}
for key, group in groups.items():
    timings=sorted(r['time_total'] for r in group if r.get('time_total') is not None)
    report[key]={'n':len(group),'failures':sum(not r['ok'] for r in group),
                 'median_s':statistics.median(timings),
                 'p95_s':timings[max(0,math.ceil(len(timings)*.95)-1)],
                 'bytes':sum(r['actual_bytes'] for r in group)}
(args.run/'statistics.json').write_text(json.dumps(report,indent=2)+'\n')
summary={'requests':len(rows),'failures':sum(not r['ok'] for r in rows),
         'bytes':sum(r['actual_bytes'] for r in rows),
         'matched_origin_ids':sum(r['id'] in origin for r in rows)}
print(json.dumps(summary,indent=2))

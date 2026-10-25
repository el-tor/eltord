#!/usr/bin/env python3
"""Check local specification navigation without network access or runtime effects."""
from pathlib import Path
import re
import sys
ROOT = Path(__file__).resolve().parents[2]
SPEC = ROOT / 'spec'
def prose(text):
    result=[]; fence=None
    for line in text.splitlines():
        match=re.match(r'^\s*(`{3,}|~{3,})',line)
        if match:
            mark=match[1][0]
            if fence is None: fence=mark
            elif fence==mark: fence=None
            result.append(''); continue
        result.append('' if fence else line)
    return '\n'.join(result), fence

def anchors(text):
    text,_=prose(text)
    titles=re.findall(r'^#{1,6}\s+(.+?)\s*#*$',text,re.M)
    titles+=re.findall(r'^([^\n]+)\n(?:=+|-+)\s*$',text,re.M)
    result=set(re.findall(r'\bid=["\x27]([^"\x27]+)',text))
    for title in titles:
        result.add(re.sub(r'[^\w\- ]','',title.lower()).replace(' ','-'))
    return result

def main():
    failures=[]; pages=list(SPEC.rglob('*.md'))
    pages += [ROOT/'TODO.md', ROOT/'readme.md']
    for page in pages:
        text=page.read_text(); clean,fence=prose(text)
        if fence: failures.append(f'{page.relative_to(ROOT)}: unclosed code fence')
        for url in re.findall(r'\]\(([^()]+)\)',clean):
            if '://' in url or url.startswith(('mailto:','data:')): continue
            name,sep,anchor=url.partition('#')
            target=(page.parent/name).resolve() if name else page
            if not target.exists(): failures.append(f'{page.relative_to(ROOT)}: missing {url}');continue
            if anchor and target.suffix=='.md' and anchor not in anchors(target.read_text()):
                failures.append(f'{page.relative_to(ROOT)}: missing anchor {url}')
    summary=(SPEC/'SUMMARY.md').read_text()
    listed={ (SPEC/m).resolve() for m in re.findall(r'\]\(([^)#]+\.md)\)',summary) }
    for page in SPEC.rglob('*.md'):
        if page.name in ('AGENTS.md','SUMMARY.md'):continue
        if page.resolve() not in listed:failures.append(f'Unlisted chapter: {page.relative_to(SPEC)}')
    ids=re.findall(r'^- \[ \] \*\*(ET-\d+) —',(ROOT/'TODO.md').read_text(),re.M)
    if len(ids)!=len(set(ids)):failures.append('Duplicate TODO IDs')
    for failure in failures: print(failure)
    if failures:return 1
    print(f'Checked {len(pages)} Markdown files, {len(listed)} chapters and {len(ids)} deferred task IDs.')
    return 0
if __name__=='__main__':sys.exit(main())

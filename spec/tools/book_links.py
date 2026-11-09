#!/usr/bin/env python3
"""Render the canonical root TODO in the book without duplicating its source."""
import json
import os
from pathlib import Path
import re
import sys
from urllib.parse import quote
if len(sys.argv)>1 and sys.argv[1]=='supports':
    sys.exit(0 if sys.argv[2]=='html' else 1)
context, book=json.load(sys.stdin)
repository=context['config'].get('output',{}).get('html',{}).get('git-repository-url','').rstrip('/')
revision=os.environ.get('GITHUB_SHA','master')
if not re.fullmatch(r'[0-9a-f]{40}',revision):revision='master'
root=Path(context['root']); src=root/'spec'; todo=root/'TODO.md'; chapter=src/'validation/backlog.md'
def rewrite(text,origin,destination):
    def replace(match):
        url=match[1]
        if '://' in url or url.startswith(('mailto:','data:')):return match[0]
        name,sep,anchor=url.partition('#')
        target=(origin.parent/name).resolve() if name else origin
        if target==todo:target=chapter
        if repository and target.is_relative_to(root) and not target.is_relative_to(src):
            return ']('+repository+'/blob/'+revision+'/'+quote(target.relative_to(root).as_posix())+sep+anchor+')'
        if target==src/'SUMMARY.md':return ']('+os.path.relpath(src/'toc.html',destination.parent)+')'
        relative=os.path.relpath(target,destination.parent)
        return ']('+relative+sep+anchor+')'
    return re.sub(r'\]\(([^()]+)\)',replace,text)
def visit(items):
    for item in items:
        c=item.get('Chapter')
        if not c:continue
        path=src/c['source_path']
        if path==chapter:
            text=todo.read_text().replace('[README TODO](readme.md#todo)', '`readme.md` under TODO (repository-only notes)')
            c['content']=rewrite(text,todo,chapter)
        else:c['content']=rewrite(c['content'],path,path)
        visit(c['sub_items'])
visit(book['sections'])
json.dump(book,sys.stdout)

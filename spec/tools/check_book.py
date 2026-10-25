#!/usr/bin/env python3
"""Validate local rendered book targets and fragments after mdbook build."""
from pathlib import Path
from html.parser import HTMLParser
from urllib.parse import urlsplit, unquote
import sys
ROOT=Path(__file__).resolve().parents[2]/'target/spec-book'
class Page(HTMLParser):
    def __init__(self):
        super().__init__(); self.urls=[]; self.ids=set()
    def handle_starttag(self,tag,attrs):
        attrs=dict(attrs)
        if 'id' in attrs:self.ids.add(attrs['id'])
        for key in ('href','src'):
            if key in attrs:self.urls.append(attrs[key])
def main():
    pages={}; failures=[]
    for file in ROOT.rglob('*.html'):
        page=Page(); page.feed(file.read_text());pages[file.resolve()]=page
    if not pages:print('No book output: run mdbook build first.');return 1
    for file,page in pages.items():
        for url in page.urls:
            parsed=urlsplit(url)
            if parsed.scheme or parsed.netloc:continue
            path=unquote(parsed.path)
            target=(ROOT/path.lstrip('/')).resolve() if path.startswith('/') else (file.parent/path).resolve() if path else file
            if target.is_dir():target=target/'index.html'
            if not target.exists():failures.append((file.relative_to(ROOT),url,'missing file'))
            elif parsed.fragment and target in pages and unquote(parsed.fragment) not in pages[target].ids:
                failures.append((file.relative_to(ROOT),url,'missing anchor'))
    for failure in failures:print(*failure,sep=': ')
    print(f'Checked {len(pages)} rendered pages; {len(failures)} broken local references.')
    return bool(failures)
if __name__=='__main__':sys.exit(main())

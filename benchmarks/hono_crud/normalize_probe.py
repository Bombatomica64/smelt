"""Normalize probe.sh output for a semantic comparison.

Header names are lower-cased and each response's header block is sorted, so two
servers that send the same headers in a different order or case compare equal.
Status lines and bodies are kept byte-for-byte. Use alongside the raw diff,
which is what "byte-for-byte" means; this view separates header spelling from
semantic differences.
"""
import sys


def normalize(lines):
    out, headers, in_headers = [], [], False
    for line in lines:
        if line.startswith('HTTP/'):
            out.append(line)
            in_headers, headers = True, []
            continue
        if in_headers:
            if line.strip() == '':
                out.extend(sorted(headers))
                out.append(line)
                in_headers = False
            else:
                name, _, value = line.partition(':')
                headers.append(f'{name.lower()}:{value}')
            continue
        out.append(line)
    if in_headers:
        out.extend(sorted(headers))
    return out


sys.stdout.writelines(normalize(open(sys.argv[1]).readlines()))

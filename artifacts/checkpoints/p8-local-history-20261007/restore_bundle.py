#!/usr/bin/env python3
"""Restore the original local history bundle from its fixed text transport."""
import argparse
import base64
import binascii
import hashlib
import json
from pathlib import Path
import sys

ENCODED_BYTES = 756821
ENCODED_SHA256 = 'f5d98c42207f9f68dd2213e25e3630584612f561d1aa604b0ed3483a629c90ad'
DECODED_BYTES = 560244
DECODED_SHA256 = '0db458b30244f383c1658db83cb97728b15b340289011d2c83d328ef69694bc4'


def read_original(source):
    with source.open('rb') as stream:
        encoded = stream.read(ENCODED_BYTES + 1)
    if len(encoded) != ENCODED_BYTES:
        raise ValueError('Base64 transport size does not match the fixed receipt')
    if hashlib.sha256(encoded).hexdigest() != ENCODED_SHA256:
        raise ValueError('Base64 transport SHA256 does not match the fixed receipt')
    decoded = base64.b64decode(encoded.replace(b'\n', b''), validate=True)
    unwrapped = base64.b64encode(decoded)
    canonical = b'\n'.join(unwrapped[i:i + 76]
                           for i in range(0, len(unwrapped), 76)) + b'\n'
    if encoded != canonical:
        raise ValueError('Expected standard Base64 with 76-column LF lines and a final LF')
    if len(decoded) != DECODED_BYTES:
        raise ValueError('Decoded bundle size does not match the original receipt')
    if hashlib.sha256(decoded).hexdigest() != DECODED_SHA256:
        raise ValueError('Decoded bundle SHA256 does not match the original receipt')
    return decoded


def main():
    here = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path,
                        default=here / 'p8-local-history.bundle',
                        help='destination; must not already exist')
    args = parser.parse_args()
    try:
        decoded = read_original(here / 'p8-local-history.bundle.b64')
        with args.output.open('xb') as stream:
            stream.write(decoded)
        with args.output.open('rb') as stream:
            restored = stream.read(DECODED_BYTES + 1)
        if len(restored) != DECODED_BYTES or hashlib.sha256(restored).hexdigest() != DECODED_SHA256:
            raise ValueError('Restored file failed the original size or SHA256 check')
    except FileExistsError:
        print('Refusing to replace an existing output file: ' + str(args.output), file=sys.stderr)
        return 2
    except (OSError, ValueError, binascii.Error) as error:
        print('Bundle restoration failed: ' + str(error), file=sys.stderr)
        return 1
    print(json.dumps({'status': 'restored', 'output': str(args.output),
                      'bytes': len(restored), 'sha256': DECODED_SHA256,
                      'exclusive_create': True, 'source_admission': 'not_granted'}, sort_keys=True))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())

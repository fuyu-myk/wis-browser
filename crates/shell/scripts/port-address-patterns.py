"""Writes crates/shell/src-tauri/src/address_field/patterns.js from Chromium's
components/autofill/core/browser/form_parsing/resources/legacy_regex_patterns.json.

    python3 crates/shell/scripts/port-address-patterns.py <legacy_regex_patterns.json> \
        crates/shell/src-tauri/src/address_field/patterns.js

Update COMMIT to the Chromium commit the JSON came from.
"""
import json
import sys

SOURCE, TARGET = sys.argv[1], sys.argv[2]
COMMIT = '7c4a2eb (2026-08-11)'
KEYS = [
    'NAME_ON_CARD', 'CREDIT_CARD_NUMBER', 'ONE_TIME_CODE',
    'SEARCH_TERM', 'ADDRESS_LOOKUP', 'ATTENTION_IGNORED', 'ADDRESS_NAME_IGNORED',
    'PHONE_COUNTRY_CODE', 'EMAIL_ADDRESS', 'PHONE', 'PHONE_AREA_CODE', 'PHONE_PREFIX',
    'PHONE_SUFFIX', 'PHONE_EXTENSION', 'ZIP_4', 'ZIP_CODE', 'COUNTRY', 'STATE',
    'ADDRESS_LINE_1', 'ADDRESS_LINE_2', 'ADDRESS_LINE_EXTRA', 'CITY', 'COMPANY_NAME',
    'NAME_IGNORED', 'HONORIFIC_PREFIX', 'MIDDLE_INITIAL', 'MIDDLE_NAME', 'LAST_NAME_SECOND',
    'LAST_NAME_FIRST', 'FIRST_NAME', 'LAST_NAME', 'FULL_NAME',
]
VARIANT = {'ZIP_4': 'AutofillSupportSplitZipCode'}
TYPES = {
    'INPUT_TEXT': 1, 'INPUT_SEARCH': 2, 'INPUT_TELEPHONE': 4, 'INPUT_NUMBER': 8,
    'INPUT_EMAIL': 16, 'SELECT_ONE': 32, 'TEXT_AREA': 64, 'INPUT_PASSWORD': 128,
}

text = open(SOURCE, encoding='utf-8').read()
data = json.loads('\n'.join(l for l in text.split('\n') if not l.lstrip().startswith('//')))

lines = []
for key in KEYS:
    rows = []
    for lang, entries in data[key].items():
        if isinstance(entries, dict):
            entries = entries.get(VARIANT.get(key, ''), [])
        for e in entries:
            if not isinstance(e, dict):
                continue
            attrs = e.get('match_field_attributes') or []
            rows.append([
                lang,
                e['positive_pattern'],
                e.get('negative_pattern') or '',
                (1 if 'LABEL' in attrs else 0) | (2 if 'NAME' in attrs else 0),
                sum(TYPES[t] for t in e.get('form_control_types') or []),
            ])
    assert rows, key
    lines.append(f'    {key}: {json.dumps(rows, ensure_ascii=False, separators=(",", ":"))}')

header = f"""// Address and contact field patterns from Chromium's
// components/autofill/core/browser/form_parsing/resources/legacy_regex_patterns.json
// at commit {COMMIT}, keeping only the keys Wis reads. Written by
// crates/shell/scripts/port-address-patterns.py.
//
// Copyright 2025 The Chromium Authors
// Use of this source code is governed by a BSD-style license that can be
// found in THIRD_PARTY_NOTICES.txt at the root of this repository.
//
// Each entry is [language, positive pattern, negative pattern or '',
// attributes (1 label, 2 name), control types (1 text, 2 search, 4 tel,
// 8 number, 16 email, 32 select, 64 textarea, 128 password)]. ZIP_4 keeps
// only its AutofillSupportSplitZipCode variant.
function addressPatterns() {{
  return {{
"""
with open(TARGET, 'w', encoding='utf-8') as out:
    out.write(header + ',\n'.join(lines) + '\n  };\n}\n')

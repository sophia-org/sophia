#!/usr/bin/env python3
"""Controls for the fixed WM row generator. Run: python3 -B tools/test_generate_wm_rows.py"""
import unittest

import generate_wm_rows as gen

FILES = (gen.ROOT / gen.SCHEMA).read_text()
SOCKET = (gen.ROOT / gen.LEGACY).read_text()


def rows(text):
    return gen.read_rows(text)


class GeneratedRows(unittest.TestCase):
    def test_checked_in_rows_are_current(self):
        header, source = gen.generate()
        self.assertEqual((gen.ROOT / gen.HEADER).read_text(), header)
        self.assertEqual((gen.ROOT / gen.SOURCE).read_text(), source)

    def test_file_rows_generate_without_socket_codec(self):
        parsed = rows(FILES)
        records = parsed["records"]
        self.assertEqual(sum(row[3] is None for row in records), 8)
        self.assertEqual(sum(row[3] is not None for row in records), 14)
        header, source = gen.render(parsed)
        self.assertIn("sophia-wm-files-v1.kdl row-layouts", header)
        for retired in ("sophia_wm_v1", "SOPHIA_WM_V1", "frame"):
            self.assertNotIn(retired, header + source)

    def test_frozen_socket_rows_match_the_file_contract(self):
        gen.check_legacy(rows(FILES), SOCKET)


class RefusedSchemas(unittest.TestCase):
    def refuse(self, text):
        with self.assertRaises(gen.SchemaError):
            rows(text)

    def edit(self, before, after, text=FILES):
        self.assertIn(before, text)
        return text.replace(before, after, 1)

    def test_missing_duplicate_and_misplaced_layouts(self):
        self.refuse(self.edit("row-layouts interface", "retired-layouts interface"))
        self.refuse(self.edit("    row-layouts interface",
                              "    row-layouts {\n    }\n    row-layouts interface"))
        self.refuse(self.edit("    row-layouts interface",
                              "    row-layouts interface-major=1 interface-revision=3 "
                              "max-outputs=16 max-surfaces=1024 max-bindings=256 {\n"
                              "        capability \"bindings\" bit=0\n    }\n"
                              "    row-layouts interface"))
        nested = self.edit("    row-layouts interface", "    body \"Wrapper\" size=0 {\n"
                           "    row-layouts interface")
        self.refuse(self.edit("\n    submit size=24", "\n    }\n    submit size=24", nested))
        self.refuse(self.edit("protocol \"sophia_wm_fs_v1\"", "protocol \"sophia_wm_v1\""))
        self.refuse(self.edit(" max-bindings=256 {", " max-bindings=256 extra=1 {"))

    def test_unknown_nodes_gates_and_inventory(self):
        self.refuse(self.edit('capability "bindings"', 'unknown "bindings"'))
        self.refuse(self.edit('gate="launch_placement"', 'gate="not_a_capability"'))
        self.refuse(self.edit('extension-record "ProjectionPresentationBinding"',
                              'record "ProjectionPresentationBinding"'))
        self.refuse(self.edit('kind=65293', 'kind=4'))
        self.refuse(self.edit('record "ProjectionOutput" transfer="projection" kind=1',
                              'record "ProjectionOutput" transfer="projection" kind=2'))
        start = FILES.index('        extension-record "ProjectionPresentationBinding"')
        end = FILES.index("        }\n", start) + len("        }\n")
        self.refuse(FILES[:start] + FILES[end:])

    def test_changed_field_shape(self):
        self.refuse(self.edit('field "focus_index" type="u32"', 'field "focus_index" type="bytes"'))
        self.refuse(self.edit('field "focus_index" type="u32" sample=3',
                              'field "focus_index" type="u32" max=4 sample=3'))
        self.refuse(self.edit('field "focus_index" type="u32" sample=3',
                              'field "focus_index" type="u32" count=2 sample=3'))
        self.refuse(self.edit('field "focus_index" type="u32" sample=3',
                              'field "focus_index" type="u32"'))
        self.refuse(self.edit('type="u8" count=32 sample="7765', 'type="u8" count=31 sample="7765'))
        self.refuse(self.edit('reserved=#true sample=0', 'reserved=#false sample=0'))
        self.refuse(self.edit('reserved=#true sample=0', 'reserved=#true sample=1'))

    def test_legacy_boolean_spelling_is_not_parsed(self):
        self.refuse(self.edit('reserved=#true sample=0', 'reserved=true sample=0'))
        self.refuse(self.edit('offset=8 nonzero=#true', 'offset=8 nonzero=true'))

    def test_drift_in_each_row_contract_dimension_is_refused(self):
        for before, after in [
            ("max-outputs=16", "max-outputs=15"),
            ("interface-revision=3", "interface-revision=4"),
            ('capability "actions" bit=1', 'capability "actions" bit=20'),
            ('outcome "committed" value=1', 'outcome "committed" value=6'),
            ('gate="launch_placement"', 'gate="actions"'),
            ('kind=1 max=16 {\n            field "output" type="u64" sample=1\n'
             '            field "generation"',
             'kind=1 max=15 {\n            field "output" type="u64" sample=1\n'
             '            field "generation"'),
            ('field "work_y" type="i32" sample=24', 'field "work_y" type="i32" sample=25'),
            ('field "focus_index" type="u32"', 'field "focus_index" type="u64"'),
        ]:
            with self.subTest(after=after):
                changed = rows(self.edit(before, after))
                with self.assertRaises(gen.SchemaError):
                    gen.check_legacy(changed, SOCKET)

    def test_missing_legacy_rows_are_refused(self):
        with self.assertRaises(gen.SchemaError):
            gen.check_legacy(rows(FILES), SOCKET.replace('"sophia_wm_v1"', '"other"', 1))


if __name__ == "__main__":
    unittest.main()

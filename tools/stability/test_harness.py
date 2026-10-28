#!/usr/bin/env python3
"""Control capture must not confuse circuit/stream IDs with status codes."""
import io
import unittest
from soak import read_reply

class Replies(unittest.TestCase):
    def test_data_rows_are_not_terminal_status(self):
        reply = b'250+circuit-status=\r\n5 BUILT test\r\n555 BUILT test\r\n12 BUILT paid\r\n.\r\n250 OK\r\n'
        result = read_reply(io.BytesIO(reply))
        self.assertIn('12 BUILT paid', result)
        self.assertTrue(result.endswith('250 OK'))

    def test_rejected_command_is_terminal(self):
        stream = io.BytesIO(b'552 Unknown circuit\r\n250 closing connection\r\n')
        self.assertEqual(read_reply(stream), '552 Unknown circuit')
        self.assertEqual(stream.readline(), b'250 closing connection\r\n')

if __name__ == '__main__':
    unittest.main()

import json
import os
import sys
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import flux_generate


class FakeComfy:
    def __init__(self, answer_after=None, in_queue=True):
        self.prompts = []
        self.deleted = []
        self.answer_after = answer_after
        self.in_queue = in_queue
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def send(self, body, kind="application/json"):
                data = body if isinstance(body, bytes) else json.dumps(body).encode()
                self.send_response(200)
                self.send_header("Content-Type", kind)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                if self.path == "/prompt":
                    pid = f"pid{len(fake.prompts)}"
                    fake.prompts.append(pid)
                    self.send({"prompt_id": pid})
                else:
                    fake.deleted.extend(body.get("delete", []))
                    self.send({})

            def do_GET(self):
                if self.path == "/queue":
                    running = [[0, p, {}] for p in fake.prompts if fake.in_queue]
                    self.send({"queue_running": [[1, "other-client-prompt", {}]], "queue_pending": running})
                elif self.path.startswith("/history/"):
                    pid = self.path.rsplit("/", 1)[1]
                    answered = fake.answer_after is not None and len(fake.prompts) > fake.answer_after
                    if answered and pid == fake.prompts[-1]:
                        self.send({pid: {"outputs": {"10": {"images": [{"filename": "a.png", "subfolder": "", "type": "output"}]}}}})
                    else:
                        self.send({})
                elif self.path.startswith("/view"):
                    self.send(b"PNGDATA", "image/png")

        self.server = HTTPServer(("127.0.0.1", 0), Handler)
        self.base = f"http://127.0.0.1:{self.server.server_port}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()


class Resilience(unittest.TestCase):
    def comfy(self, fake, **kw):
        return flux_generate.Comfy(None, base=fake.base, poll=0.02, **kw)

    def test_never_answering_server_times_out_and_skips(self):
        fake = FakeComfy()
        self.addCleanup(fake.close)
        result = self.comfy(fake, timeout=0.3).run({}, attempts=2)
        self.assertIsNone(result)
        self.assertEqual(len(fake.prompts), 2)
        self.assertEqual(fake.deleted, fake.prompts)

    def test_resubmits_once_then_succeeds(self):
        fake = FakeComfy(answer_after=1)
        self.addCleanup(fake.close)
        result = self.comfy(fake, timeout=0.3).run({}, attempts=2)
        self.assertEqual(result, b"PNGDATA")
        self.assertEqual(len(fake.prompts), 2)

    def test_prompt_missing_from_queue_is_detected_before_timeout(self):
        fake = FakeComfy(in_queue=False)
        self.addCleanup(fake.close)
        result = self.comfy(fake, timeout=30, lost_grace=0.2).run({}, attempts=2)
        self.assertIsNone(result)
        self.assertEqual(len(fake.prompts), 2)

    def test_only_our_prompts_are_deleted(self):
        fake = FakeComfy()
        self.addCleanup(fake.close)
        self.comfy(fake, timeout=0.2).run({}, attempts=1)
        self.assertNotIn("other-client-prompt", fake.deleted)


if __name__ == "__main__":
    unittest.main()

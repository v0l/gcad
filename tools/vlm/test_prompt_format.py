import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import prompt_format


class FakeProcessor:
    def apply_chat_template(self, messages, tokenize, add_generation_prompt):
        return "<chat>" + str(len(messages[0]["content"])) + "</chat>"


class FakeTokenizer:
    def convert_tokens_to_ids(self, token):
        return {"<|endoftext|>": 7, "<|im_end|>": 9}[token]


class PromptFormatTest(unittest.TestCase):
    def test_base_models_use_plain_format(self):
        self.assertEqual(prompt_format.format_for("Qwen/Qwen3.5-4B-Base"), "plain")
        self.assertEqual(prompt_format.format_for("Qwen/Qwen3-VL-4B-Instruct"), "chat")

    def test_plain_prompt_is_image_tokens_then_text(self):
        text = prompt_format.prompt_text(FakeProcessor(), "plain", 4, "Write it.")
        self.assertEqual(text.count("<|image_pad|>"), 4)
        self.assertTrue(text.startswith(prompt_format.IMAGE_TOKENS * 4 + "Write it."))
        self.assertNotIn("<|im_start|>", text)

    def test_chat_prompt_uses_template(self):
        text = prompt_format.prompt_text(FakeProcessor(), "chat", 4, "Write it.")
        self.assertEqual(text, "<chat>5</chat>")

    def test_end_markers(self):
        self.assertEqual(prompt_format.end_marker("plain"), "<|endoftext|>")
        self.assertEqual(prompt_format.end_marker("chat"), "<|im_end|>")
        self.assertEqual(prompt_format.stop_ids(FakeTokenizer(), "plain"), [7])
        self.assertEqual(prompt_format.stop_ids(FakeTokenizer(), "chat"), [9])


if __name__ == "__main__":
    unittest.main()

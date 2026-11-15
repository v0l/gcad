import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import agent_format
import overlap
import train_agent

START, END, ASSISTANT = 1, 2, 3


class Fake:
    def __init__(self):
        self.vocab = {}

    def id(self, word):
        return self.vocab.setdefault(word, 10 + len(self.vocab))


def tokenize(text, fake):
    out = []
    for tag in ("<|im_start|>", "<|im_end|>", "<|image_pad|>"):
        text = text.replace(tag, f" {tag} ")
    for word in text.replace("\n", " \n ").split(" "):
        if not word:
            continue
        special = {"<|im_start|>": START, "<|im_end|>": END, "assistant": ASSISTANT}
        out.append(special.get(word) or fake.id(word))
    return out


EXAMPLE = {
    "system": "sys",
    "messages": [
        {"role": "user", "content": [{"type": "text", "text": "Make a plate"}]},
        {"role": "assistant", "content": [{"type": "text", "text": "writing"}],
         "tool_calls": [{"id": "c1", "name": "gcad", "arguments": "{\"args\": [\"check\"]}"}]},
        {"role": "tool", "tool_call_id": "c1", "content": [{"type": "text", "text": "ok"}]},
        {"role": "user", "content": [{"type": "image", "image": "/tmp/a.png"}, {"type": "text", "text": "look"}]},
        {"role": "assistant", "content": [{"type": "text", "text": "done"}]},
    ],
}


class AgentFormat(unittest.TestCase):
    def test_render_places_images_in_order_where_they_were_viewed(self):
        text, images = agent_format.render(EXAMPLE)
        self.assertEqual(images, ["/tmp/a.png"])
        self.assertEqual(text.count("<|image_pad|>"), 1)
        self.assertLess(text.index("tool_response"), text.index("<|image_pad|>"))
        self.assertLess(text.index("<|image_pad|>"), text.rindex("done"))

    def test_render_writes_tool_calls_as_json_in_assistant_turn(self):
        text, _ = agent_format.render(EXAMPLE)
        self.assertIn('<tool_call>\n{"name": "gcad", "arguments": {"args": ["check"]}}\n</tool_call>', text)

    def test_loss_only_on_assistant_turns(self):
        fake = Fake()
        text, _ = agent_format.render(EXAMPLE)
        ids = tokenize(text, fake)
        labels = agent_format.assistant_labels(ids, START, END, ASSISTANT)
        trained = [i for i, l in enumerate(labels) if l != -100]
        self.assertTrue(all(labels[i] == ids[i] for i in trained))
        words = {v: k for k, v in fake.vocab.items()}
        words.update({START: "<s>", END: "<e>", ASSISTANT: "assistant"})
        trained_words = {words[ids[i]] for i in trained}
        self.assertIn("writing", trained_words)
        self.assertIn("done", trained_words)
        self.assertIn("<e>", trained_words)
        for masked in ("Make", "plate", "ok", "sys", "look", "<|image_pad|>"):
            self.assertNotIn(masked, trained_words)
        self.assertNotIn(ASSISTANT, [ids[i] for i in trained])
        self.assertNotIn(START, [ids[i] for i in trained])
        assistant_turns = sum(1 for i in range(len(ids) - 1) if ids[i] == START and ids[i + 1] == ASSISTANT)
        self.assertEqual(assistant_turns, 2)
        self.assertEqual(sum(1 for i in trained if ids[i] == END), 2)

    def test_training_data_in_minds_eye_is_refused(self):
        with self.assertRaises(ValueError):
            train_agent.load_examples(overlap.MINDS_EYE_DIR / "results" / "x.jsonl")


if __name__ == "__main__":
    unittest.main()

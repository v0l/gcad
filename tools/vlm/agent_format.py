import json

IMAGE_TOKENS = "<|vision_start|><|image_pad|><|vision_end|>"
START = "<|im_start|>"
END = "<|im_end|>"


def render_content(content):
    if isinstance(content, str):
        return content, []
    text, images = "", []
    for part in content:
        if part["type"] == "image":
            text += IMAGE_TOKENS
            images.append(part["image"])
        else:
            text += part["text"]
    return text, images


def render_tool_calls(calls):
    out = ""
    for c in calls:
        arguments = c["arguments"]
        if isinstance(arguments, str):
            arguments = json.loads(arguments or "{}")
        out += "\n<tool_call>\n" + json.dumps({"name": c["name"], "arguments": arguments}) + "\n</tool_call>"
    return out


def render(example):
    chunks = []
    images = []
    if example.get("system"):
        chunks.append(f"{START}system\n{example['system']}{END}\n")
    for m in example["messages"]:
        text, found = render_content(m.get("content") or [])
        images.extend(found)
        if m["role"] == "assistant":
            chunks.append(f"{START}assistant\n{text}{render_tool_calls(m.get('tool_calls') or [])}{END}\n")
        elif m["role"] == "tool":
            chunks.append(f"{START}user\n<tool_response>\n{text}\n</tool_response>{END}\n")
        else:
            chunks.append(f"{START}user\n{text}{END}\n")
    return "".join(chunks), images


def assistant_labels(ids, start_id, end_id, assistant_id, header=2):
    labels = [-100] * len(ids)
    i = 0
    while i < len(ids):
        if ids[i] == start_id and i + 1 < len(ids) and ids[i + 1] == assistant_id:
            j = i + 1 + header
            while j < len(ids):
                labels[j] = ids[j]
                if ids[j] == end_id:
                    break
                j += 1
            i = j
        i += 1
    return labels


def special_ids(tokenizer):
    ids = tokenizer.convert_tokens_to_ids
    return ids(START), ids(END), ids("assistant")

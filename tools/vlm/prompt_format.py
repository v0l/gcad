IMAGE_TOKENS = "<|vision_start|><|image_pad|><|vision_end|>"
END_OF_TEXT = "<|endoftext|>"
END_OF_TURN = "<|im_end|>"


def format_for(base):
    return "plain" if "base" in base.lower() else "chat"


def prompt_text(processor, fmt, n_images, text):
    if fmt == "plain":
        return IMAGE_TOKENS * n_images + text + "\n\n"
    content = [{"type": "image"} for _ in range(n_images)] + [{"type": "text", "text": text}]
    return processor.apply_chat_template([{"role": "user", "content": content}], tokenize=False, add_generation_prompt=True)


def end_marker(fmt):
    return END_OF_TEXT if fmt == "plain" else END_OF_TURN


def stop_ids(tokenizer, fmt):
    return [tokenizer.convert_tokens_to_ids(end_marker(fmt))]

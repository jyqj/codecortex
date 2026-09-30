DEFAULT_TIMEOUT = 30


def load_settings(overrides):
    return {"timeout": overrides.get("timeout", DEFAULT_TIMEOUT)}

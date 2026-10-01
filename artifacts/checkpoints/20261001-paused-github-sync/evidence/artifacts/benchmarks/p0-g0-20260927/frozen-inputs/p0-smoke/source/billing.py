"""Billing renewals are distinct from authentication sessions."""

def renew_subscription(account, months):
    """Extend a paid subscription; does not issue or refresh session tokens."""
    return {"account": account, "months": months, "invoice_required": True}


def validate_amount(amount):
    if amount <= 0:
        raise ValueError("amount must be positive")
    return amount

from typing import Optional, List


class BaseModel:
    """Abstract base for all domain models."""

    def __init__(self, id: int):
        self.id: int = id

    def to_dict(self) -> dict:
        return {"id": self.id}

    @staticmethod
    def validate_id(value: int) -> bool:
        return isinstance(value, int) and value > 0


class User(BaseModel):
    """Standard user account."""

    def __init__(self, id: int, name: str, email: Optional[str] = None):
        super().__init__(id)
        self.name: str = name
        self.email: Optional[str] = email

    def to_dict(self) -> dict:
        base = super().to_dict()
        base.update({"name": self.name, "email": self.email})
        return base

    @classmethod
    def from_dict(cls, data: dict) -> "User":
        return cls(id=data["id"], name=data["name"], email=data.get("email"))

    def display_name(self) -> str:
        return self.name if self.name else f"User#{self.id}"


class AdminUser(User):
    """Privileged user with role-based access."""

    def __init__(self, id: int, name: str, role: str = "admin"):
        super().__init__(id, name)
        self.role: str = role

    def to_dict(self) -> dict:
        base = super().to_dict()
        base["role"] = self.role
        return base

    def promote(self, new_role: str) -> None:
        if new_role not in ("admin", "superadmin"):
            raise ValueError(f"Invalid role: {new_role}")
        self.role = new_role


class AuditLog:
    """Unused model — no other module references it."""

    def __init__(self, entries: Optional[List[str]] = None):
        self.entries: List[str] = entries or []

    def add(self, message: str) -> None:
        self.entries.append(message)

    def clear(self) -> None:
        self.entries.clear()

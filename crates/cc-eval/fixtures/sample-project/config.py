import os

DATABASE_URL = os.environ.get("DATABASE_URL", "sqlite:///db.sqlite3")
SECRET_KEY = os.environ["SECRET_KEY"]
DEBUG = os.environ.get("DEBUG", "false").lower() == "true"
API_KEY = os.getenv("API_KEY")

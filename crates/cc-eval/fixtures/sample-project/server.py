from flask import Flask, jsonify
from app import get_user, create_user

app = Flask(__name__)

@app.route('/users/<int:user_id>', methods=['GET'])
def api_get_user(user_id):
    return jsonify(get_user(user_id))

@app.route('/users', methods=['POST'])
def api_create_user():
    return jsonify(create_user("new_user"))

@app.route('/health', methods=['GET'])
def health_check():
    return jsonify({"status": "ok"})

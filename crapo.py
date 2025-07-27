from flask import Flask, request

app = Flask(__name__)

@app.route('/', methods=['POST'])
def receive():
    print("Received POST data:", request.get_json())
    return {"status": "received"}, 200

if __name__ == "__main__":
    app.run(host="127.0.0.1", port=5001)

from flask import Flask, request

app = Flask(__name__)

@app.route('/', methods=['POST'])
def catch_post():
    data = request.get_json()
    print("Received POST data:", data)
    return {"status": "ok"}, 200

if __name__ == '__main__':
    app.run(port=3001)

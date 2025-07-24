from flask import Flask, request
import subprocess
import base58
import threading
import requests
import os

app = Flask(__name__)

base58_chrset = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'

def grind(base, owner, return_url, uuid, prefix, suffix):
    # This function will call the Rust binary with the provided parameters
    command = [
        './vanity',
        'grind',
        '--base', base,
        '--owner', owner,
        '--return-url', return_url,
        '--uuid', uuid
    ]

    if prefix:
        command += ['--prefix', prefix]
    if suffix:
        command += ['--suffix', suffix]

    print(f"Executing command: {' '.join(command)}")
    
    # Send command to run the Rust binary then shut down the python server
    try:
        subprocess.run(command, check=True)
    except subprocess.CalledProcessError as e:
        error_message = {"status": "0xFF", "message": f"Error executing command: {str(e)}"}
        try:
            requests.post(return_url, json=error_message)
        except Exception as post_error:
            print(f"Failed to send error message to return_url: {str(post_error)}")
    
    print("Rust binary executed successfully")
    os._exit(0)


@app.route('/', methods=['POST'])
def receive():
    # You need to recive these parameters from the request

    # make sure the data is in JSON format
    data = request.get_json()
    if not data:
        return "Invalid JSON", 400
    
    # Extract the parameters from the JSON data
    base = data.get('base')
    owner = data.get('owner')

    # Ensure both parameters are provided
    if not base or not owner:
        return {"status": "0x01", "message": "Missing 'base' or 'owner' parameter"}, 400
        
    # Make sure the base is a valid base58 string with a length of 32 (for solana addresses)
    try:
        decoded_base = base58.b58decode(base)
        if len(decoded_base) != 32:
            return {"status": "0x02", "message": "Invalid base58 string length"}, 400
    except Exception as e:
        return {"status": "0x03", "message": f"Invalid base58 string: {str(e)}"}, 400

    # Make sure we get a return url
    return_url = data.get('return_url')
    if not return_url:
        return {"status": "0x04", "message": "Missing 'return_url' parameter"}, 400
    
    # Make sure we get a uuid
    uuid = data.get('uuid')
    if not uuid:
        return {"status": "0x05", "message": "Missing 'uuid' parameter"}, 400
    
    # Now we also need to get a prefix and suffix
    prefix = data.get('prefix')
    suffix = data.get('suffix')
    
    if not prefix and not suffix:
        return {"status": "0x06", "message": "Missing 'prefix' and 'suffix' parameters"}, 400
    
    # make sure the characters in the prefix and suffix are valid base58 characters
    if prefix:
        for char in prefix:
            if char not in base58_chrset:
                return {"status": "0x07", "message": f"Invalid character '{char}' in prefix"}, 400
    if suffix:
        for char in suffix:
            if char not in base58_chrset:
                return {"status": "0x08", "message": f"Invalid character '{char}' in suffix"}, 400
            
    # Make sure the prefix and suffix are not too long
    if prefix and len(prefix) > 5:
        return {"status": "0x09", "message": "Prefix too long, max 5 characters"}, 400
    if suffix and len(suffix) > 5:
        return {"status": "0x0A", "message": "Suffix too long, max 5 characters"}, 400

    
    # Start the intesive rust execution
    thread = threading.Thread(target=grind, args=(base, owner, return_url, uuid, prefix, suffix))
    thread.daemon = True
    thread.start()

    return {"status": "0x00", "message": "Request received, processing started"}, 202
    

app.run(host='0.0.0.0', port=8080)

import socket
import json
import time

SOCK = "/tmp/whisper-wayland-mcp.sock"

def main():
    print(f"Connecting to {SOCK}...")
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
            s.connect(SOCK)
            
            # Use makefile to easily read line by line
            f = s.makefile('rw', encoding='utf-8')
            
            def rpc(method, params=None, rpc_id=1):
                req = {"jsonrpc": "2.0", "id": rpc_id, "method": method, "params": params or {}}
                print(f"-> {method}: {json.dumps(params)}")
                f.write(json.dumps(req) + "\n")
                f.flush()
                
                line = f.readline()
                if not line:
                    raise ConnectionError("Connection closed by server")
                resp = json.loads(line)
                
                # Print truncated response for cleaner logs
                resp_str = str(resp)
                if len(resp_str) > 100:
                    resp_str = resp_str[:100] + "..."
                print(f"<- {resp_str}")
                return resp

            # Handshake
            print("--- Initializing ---")
            rpc("initialize", rpc_id=1)
            
            # Send initialized notification
            init_notif = {"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}
            f.write(json.dumps(init_notif) + "\n")
            f.flush()
            print("-> notifications/initialized")

            # 1. Ask the user to say something
            print("\n--- 1. Sending TTS Prompt ---")
            rpc("tools/call", {
                "name": "speak_text", 
                "arguments": {"text": "Please say something."}
            }, rpc_id=2)
            
            # Give TTS a brief moment to start before opening the mic
            time.sleep(1)

            # 2. Listen to what they say
            print("\n--- 2. Opening Microphone ---")
            resp = rpc("tools/call", {
                "name": "transcribe_voice", 
                "arguments": {"timeout_seconds": 15.0}
            }, rpc_id=3)
            
            # Extract transcript
            if "result" in resp and not resp.get("error"):
                transcript = resp["result"]["content"][0]["text"]
                print(f"\n[Transcript Received]: {transcript}")
                
                # Formulate reply
                if transcript and transcript != "(no speech detected)":
                    reply_text = f"You said: {transcript}"
                else:
                    reply_text = "I didn't hear you say anything."
                
                # 3. Reply with what they said
                print("\n--- 3. Sending TTS Reply ---")
                rpc("tools/call", {
                    "name": "speak_text", 
                    "arguments": {"text": reply_text}
                }, rpc_id=4)
                
                # Give the socket a moment so the TTS command goes through before closing
                time.sleep(1)
            else:
                print(f"Error during transcription: {resp.get('error', 'Unknown Error')}")

    except FileNotFoundError:
        print(f"Error: Socket {SOCK} not found.")
        print("Please ensure Whisper-Wayland is running and the MCP Server is enabled in Settings.")
    except Exception as e:
        print(f"Failed to communicate with MCP server: {e}")

if __name__ == "__main__":
    main()

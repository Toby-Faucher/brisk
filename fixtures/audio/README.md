# Audio samples: READ BEFORE COMMITTING

`rikey.hex` is `<rikey, 32 hex> <rikeyid, 8 hex>`: the AES key of ONE finished streaming session. `brisk-probe stream` draws a fresh random rikey for every launch and the session was cancelled right after recording, so the key protects nothing else (not a secret in practice). It is written only with `--record-samples`. It must NEVER be a real long-lived key: never copy a paired identity, a pairing secret or anything from ~/.config/brisk here.

NNN.bin are raw audio datagrams as received (data packets: AES-128-CBC ciphertext; FEC packets: parity over the ciphertext).

// Generated synthetic.json with the did:plc reference implementation
// (npm install @did-plc/lib@0.0.4), from the real audit log beside it:
//   node synthetic.js ewvi7nxzyoun6zhxrhs64oiz.audit.json > synthetic.json
// Keys are random, so a rerun yields a different but equivalent file.
const plc = require('@did-plc/lib')
const { EcdsaKeypair, Secp256k1Keypair } = require('@atproto/crypto')
const { cidForCbor } = require('@atproto/common')
const fs = require('fs')

const K256_N = BigInt('0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141')
const P256_N = BigInt('0xffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551')
const b64u = (b) => Buffer.from(b).toString('base64url')
const unb64u = (s) => Buffer.from(s, 'base64url')

// The same signature with s replaced by n - s: valid ECDSA, but high-S.
const highS = (sig, n) => {
  const bytes = unb64u(sig)
  const s = BigInt('0x' + bytes.subarray(32).toString('hex'))
  const flipped = Buffer.from((n - s).toString(16).padStart(64, '0'), 'hex')
  return b64u(Buffer.concat([bytes.subarray(0, 32), flipped]))
}

const entry = async (did, operation, createdAt, nullified = false) => ({
  did, operation, cid: (await cidForCbor(operation)).toString(), nullified, createdAt,
})

;(async () => {
  const recovery = await Secp256k1Keypair.create()
  const rotation = await Secp256k1Keypair.create()
  const attacker = await Secp256k1Keypair.create()
  const signing = await Secp256k1Keypair.create()
  const { op: genesis, did } = await plc.createOp({
    signingKey: signing.did(), handle: 'alice.test', pds: 'https://pds.example',
    rotationKeys: [recovery.did(), rotation.did()], signer: rotation,
  })
  const update = await plc.updateHandleOp(genesis, rotation, 'mallory.test')
  const recover = (signer) => plc.updateRotationKeysOp(genesis, signer, [recovery.did()])
  const t0 = '2024-01-01T00:00:00.000Z'
  const t1 = '2024-01-02T00:00:00.000Z'
  const g = await entry(did, genesis, t0)
  const out = { did, keys: { recovery: recovery.did(), rotation: rotation.did(), attacker: attacker.did() } }

  out.recovery = [g, await entry(did, update, t1, true),
    await entry(did, await recover(recovery), '2024-01-04T23:59:59.000Z')]
  out.lateRecovery = [g, await entry(did, update, t1, true),
    await entry(did, await recover(recovery), '2024-01-05T00:00:00.001Z')]
  out.sameAuthorityRecovery = [g, await entry(did, update, t1, true),
    await entry(did, await recover(rotation), '2024-01-02T01:00:00.000Z')]
  out.tombstone = [g, await entry(did, await plc.tombstoneOp(await cidForCbor(genesis), rotation), t1)]
  out.attacker = [g, await entry(did, await plc.updateHandleOp(genesis, attacker, 'mallory.test'), t1)]
  out.nonMonotonic = [g, await entry(did, update, '2023-12-31T00:00:00.000Z')]

  // WebCrypto P-256 signing does not normalize s, so retry until both
  // signatures are low-S, as did:plc v0.2.1+ requires.
  const isLowS = (sig) => BigInt('0x' + unb64u(sig).subarray(32).toString('hex')) <= P256_N / 2n
  let p256, p256Genesis, p256Did, p256Update
  do {
    p256 = await EcdsaKeypair.create()
    ;({ op: p256Genesis, did: p256Did } = await plc.createOp({
      signingKey: p256.did(), handle: 'carol.test', pds: 'https://pds.example',
      rotationKeys: [p256.did()], signer: p256,
    }))
    p256Update = await plc.updateHandleOp(p256Genesis, p256, 'dave.test')
  } while (!isLowS(p256Genesis.sig) || !isLowS(p256Update.sig))
  const p256HighSUpdate = { ...p256Update, sig: highS(p256Update.sig, P256_N) }
  out.p256Did = p256Did
  out.p256 = [await entry(p256Did, p256Genesis, t0), await entry(p256Did, p256Update, t1)]
  out.p256HighS = [out.p256[0], await entry(p256Did, p256HighSUpdate, t1)]

  const real = JSON.parse(fs.readFileSync(process.argv[2]))
  const last = { ...real.at(-1).operation }
  const highSOp = { ...last, sig: highS(last.sig, K256_N) }
  out.highS = [...real.slice(0, -1), { ...real.at(-1), operation: highSOp, cid: (await cidForCbor(highSOp)).toString() }]
  const chars = last.sig.split('')
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_'
  chars[chars.length - 1] = alphabet[alphabet.indexOf(chars.at(-1)) | 1]
  const looseOp = { ...last, sig: chars.join('') }
  out.nonCanonicalSig = [...real.slice(0, -1), { ...real.at(-1), operation: looseOp, cid: (await cidForCbor(looseOp)).toString() }]
  console.log(JSON.stringify(out, null, 2))
})().catch((e) => { console.error(e); process.exit(1) })

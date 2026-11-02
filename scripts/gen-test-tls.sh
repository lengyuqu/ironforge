#!/usr/bin/env bash
# Generate throwaway TLS material for tests into the directory given as $1:
#
#   ec.crt           self-signed certificate (X.509 v3, SAN ironforge-test.invalid)
#   ec-pkcs8.key     P-256 key, PKCS#8   (BEGIN PRIVATE KEY)
#   ec-sec1.key      the same key, SEC1  (BEGIN EC PRIVATE KEY)
#   rsa.crt          self-signed certificate for rsa-pkcs1.key
#   rsa-pkcs1.key    RSA 2048 key, PKCS#1 (BEGIN RSA PRIVATE KEY)
#
# Used by the `load_tls_config` unit tests and scripts/git-protocol-e2e.sh. The
# keys are created on demand and never committed; they protect nothing.
set -euo pipefail

dir="${1:?usage: gen-test-tls.sh <output-dir>}"
mkdir -p "${dir}"
cd "${dir}"

cat >openssl.cnf <<'CNF'
[req]
distinguished_name = dn
x509_extensions = v3
prompt = no
[dn]
CN = ironforge-test.invalid
[v3]
subjectAltName = DNS:ironforge-test.invalid
basicConstraints = critical,CA:FALSE
keyUsage = digitalSignature,keyEncipherment
extendedKeyUsage = serverAuth
CNF

# EC key: `ecparam -genkey` writes SEC1 on both OpenSSL 3 and LibreSSL.
openssl ecparam -name prime256v1 -genkey -noout -out ec-sec1.key
openssl pkcs8 -topk8 -nocrypt -in ec-sec1.key -out ec-pkcs8.key
openssl req -x509 -new -key ec-sec1.key -sha256 -days 3650 -config openssl.cnf -out ec.crt

# RSA key: OpenSSL 3 writes PKCS#8 by default and needs `-traditional` for
# PKCS#1; LibreSSL writes PKCS#1 already and does not know the flag.
openssl genrsa -out rsa-any.key 2048 2>/dev/null
if openssl rsa -in rsa-any.key -traditional -out rsa-pkcs1.key 2>/dev/null; then
  :
else
  openssl rsa -in rsa-any.key -out rsa-pkcs1.key 2>/dev/null
fi
rm -f rsa-any.key
openssl req -x509 -new -key rsa-pkcs1.key -sha256 -days 3650 -config openssl.cnf -out rsa.crt

rm -f openssl.cnf

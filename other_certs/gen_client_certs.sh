#!/bin/bash

# Simple script
# have to manually add email addresses
# but it is quick
#

echo "=====================first======================================="
openssl ecparam -out first.key -name prime256v1 -genkey

openssl req -new -key first.key -out first.csr -sha256

openssl ca -keyfile private/ec-cakey.pem -cert cert/ec-cacert.pem -in first.csr -out first.crt -config ./openssl.cnf -extfile client_ext.cnf

openssl verify -CAfile cert/ec-cacert.pem first.crt


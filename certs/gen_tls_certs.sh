#!/bin/bash

rm -rf *.pem *.csr *.key serial index.txt* cert private

mkdir private cert

touch index.txt
echo 01 > serial

echo "==============CA GEN================="

openssl ecparam -out private/ec-cakey.pem -name prime256v1 -genkey

openssl ecparam -in private/ec-cakey.pem -text -noout

openssl req -new -x509 -days 3650 -config ./openssl.cnf -extensions v3_ca -key private/ec-cakey.pem -out cert/ec-cacert.pem

openssl x509 -noout -text -in cert/ec-cacert.pem | grep -i algorithm

echo "==============SERVER GEN================="

openssl ecparam -out server.key -name prime256v1 -genkey


openssl req -new -key server.key -out server.csr -sha256

openssl ca -keyfile private/ec-cakey.pem -cert cert/ec-cacert.pem -in server.csr -out server.crt -config ./openssl.cnf -extfile server_ext.cnf

openssl verify -CAfile cert/ec-cacert.pem server.crt

cat server.crt cert/ec-cacert.pem > server.pem

echo "==============CLIENT GEN================="

openssl ecparam -out client.key -name prime256v1 -genkey

openssl req -new -key client.key -out client.csr -sha256

openssl ca -keyfile private/ec-cakey.pem -cert cert/ec-cacert.pem -in client.csr -out client.crt -config ./openssl.cnf -extfile client_ext.cnf

openssl verify -CAfile cert/ec-cacert.pem client.crt


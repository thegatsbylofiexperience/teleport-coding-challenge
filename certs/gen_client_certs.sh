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



echo "=====================second======================================="
openssl ecparam -out second.key -name prime256v1 -genkey

openssl req -new -key second.key -out second.csr -sha256

openssl ca -keyfile private/ec-cakey.pem -cert cert/ec-cacert.pem -in second.csr -out second.crt -config ./openssl.cnf -extfile client_ext.cnf

openssl verify -CAfile cert/ec-cacert.pem second.crt



echo "=====================third======================================="
openssl ecparam -out third.key -name prime256v1 -genkey

openssl req -new -key third.key -out third.csr -sha256

openssl ca -keyfile private/ec-cakey.pem -cert cert/ec-cacert.pem -in third.csr -out third.crt -config ./openssl.cnf -extfile client_ext.cnf

openssl verify -CAfile cert/ec-cacert.pem third.crt



echo "=====================fourth======================================="
openssl ecparam -out fourth.key -name prime256v1 -genkey

openssl req -new -key fourth.key -out fourth.csr -sha256

openssl ca -keyfile private/ec-cakey.pem -cert cert/ec-cacert.pem -in fourth.csr -out fourth.crt -config ./openssl.cnf -extfile client_ext.cnf

openssl verify -CAfile cert/ec-cacert.pem fourth.crt

#echo "=====================fifth======================================="
#openssl ecparam -out fifth.key -name prime256v1 -genkey

#openssl req -new -key fifth.key -out fifth.csr -sha256

#openssl ca -keyfile private/bad-ec-cakey.pem -cert cert/bad-ec-cacert.pem -in fifth.csr -out fifth.crt -config ./openssl.cnf -extfile client_ext.cnf

#openssl verify -CAfile cert/ec-cacert.pem fifth.crt


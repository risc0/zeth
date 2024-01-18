#!/bin/bash

set -x
openssl genrsa -out ca.key 4096
openssl req -new -key ca.key -x509 -out ca.crt -days 36500 -subj "/CN=RootCA"
openssl req -new -nodes -newkey rsa:4096 -keyout user.key -out user.req -batch -subj "/CN=mbedtls.example"
openssl x509 -req -in user.req -CA ca.crt -CAkey ca.key -CAcreateserial -out user.crt -days 36500 -sha256

faketime '2008-12-24 08:15:42' openssl req -new -nodes -newkey rsa:4096 -keyout expired.key -out expired.req -batch -subj "/CN=ExpiredNode"
faketime '2008-12-24 08:15:42' openssl x509 -req -in expired.req -CA ca.crt -CAkey ca.key -CAcreateserial -out expired.crt -days 1 -sha256
rm -f *.req
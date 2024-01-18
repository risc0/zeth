/* Copyright (c) Fortanix, Inc.
 *
 * Licensed under the GNU General Public License, version 2 <LICENSE-GPL or
 * https://www.gnu.org/licenses/gpl-2.0.html> or the Apache License, Version
 * 2.0 <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0>, at your
 * option. This file may not be copied, modified, or distributed except
 * according to those terms. */

#![allow(dead_code)]

pub const PEM_SELF_SIGNED_KEY: &'static [u8] = b"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDFjAgmCJUmKAQ/
OAg0MBh3E2+l5asSHdBNmTm0gr3vmnmFcUqlIpUG3BGd85o0c9X5qnxBKJafTJLu
2xRqjx1TMlBdtVpP0CXy5qPYwvO8UWIGyrsniy8GfpDjXGkUFbm91Cw1c/lCD7R1
6lLHK+7Npq9oxpk3KfMHivQorFd31byo0VxZv/sFYViCbDtOYmMifQX/qkqsbvkx
SuPklzpxAxF824mtKMRimwGQbZ4tbLlAFNugO02eV0Hq8xHxfbmNrblSqIy68/Ud
jg4Y9feFi8NVfYg/rsFjuL+Fv/3dLBBhaMffyV9J0eULXgVw5ZXNaQgKb6sSBQqi
U3LftHDTAgMBAAECggEBAKzBKN8Z4lTb6drfRU1eQgbgGGMb1d6h8+fod25EZ5WB
oYPw7zY6Z9j32vAmeFQmeJk9XiwdMptce6ImNFR7k0mOVnmcfr4NaSJiUCbfVgb5
pKAL6l9KeHVVeZ9a0Qmfdi9rvL2CDhiXY1k68ej7onp1qjAWfSagqMeP3LU1Acjo
tYnt42QNa/x4spOCx9EoMuKrEiBNYoll7lW6iuIqTO9Oodkh7ZHVEYNe3y4RHIpj
QmMxVrjt9Pe26cesNajkM2OWMxZW8MEeyL7DqUenxNluRrMG2lP5ZtEBuDFRTWEL
xrh89UQcFN0MZPL+HmMunS+ztu0vOh2UQw8zORSw+AECgYEA/DNK/kTJeRC0N4xh
ErcwTUBx2vtYdD/lWo4dRBanw218mXnzu25l8CjOQm0OircELy/UG0eBGCGXaxhh
H274KQqgM7ibSJHTP2J17wfbS3PxIgF75uf7UNP0M2yX/t2bjgfCsaqNckHBIrxN
Ym/FWUN884zrgapaYtiPzjTUF3MCgYEAyIXyguElCI7iYIN0qreSHvt+2qKyKhVO
6NdPf19ZvhT7vk03P6YXt/VCg+eNDeh5EBwHUWG2JQYznIK2jRLFwoFLdIlIjapq
kG9s2NWQ99HpY0mnhTUFptTEpiuyFDbEyhXhWBja5zOn1ZuqW/V9bgCglGoxOckV
2vGv4YX0KSECgYEA0YUTcnZXItr7vYJESzYhTKyTaieR7tH+iuKx8ZUYvsTA1Qh5
smcfDQv5fzn28MrnEQSdJCSdXRzbHL/eQC0CwaXwPcfKSdnMNEZqT7CpQOALngK5
mrVzFk1f/TDkfXpB9xb/anaUmC2EdIUXjQXqYCQvNG8IYGrUOHZN0jQVV30CgYAV
HW209GpG5WzXBuChHWVol8j60sj5/3ZotEttuSelCWac2lqn/CBhQZU4eIh033bo
CFuI6UYZzfZfU7BPWJu0aJL+eXpHWJuSC/mlN4/lWJg/2UCnmTa4I411hgJheIbu
VLF+6lcao2jX6GVe+5GypKREHI6EbDU98dc4YzeboQKBgF/R6wCxhVt9kqSLYeGq
nGQmHqj2/0m7M9O/QS5a4L/3Oyu5YyNuPR6OBMdjivOdz4RwUi6+o9IEepzmN1cV
okcspBUohwqnqHwvdiQjB+RygIpmnXhchXxRok3wc745S1NBCbAL5V3sa6/61/1C
YLT4mPYORlR4AgzvpNOJiI3T
-----END PRIVATE KEY-----\0";

pub const PEM_SELF_SIGNED_CERT_SUBJECT: &'static str = "CN=mbedtls.example";

pub const PEM_SELF_SIGNED_CERT: &'static [u8] = b"-----BEGIN CERTIFICATE-----
MIIDCTCCAfGgAwIBAgIJALWh9vlifeRuMA0GCSqGSIb3DQEBCwUAMBoxGDAWBgNV
BAMTD21iZWR0bHMuZXhhbXBsZTAgFw0xODExMjMwNTQ5MTBaGA8yMTAwMDEwMTA1
NDkxMFowGjEYMBYGA1UEAxMPbWJlZHRscy5leGFtcGxlMIIBIjANBgkqhkiG9w0B
AQEFAAOCAQ8AMIIBCgKCAQEAxYwIJgiVJigEPzgINDAYdxNvpeWrEh3QTZk5tIK9
75p5hXFKpSKVBtwRnfOaNHPV+ap8QSiWn0yS7tsUao8dUzJQXbVaT9Al8uaj2MLz
vFFiBsq7J4svBn6Q41xpFBW5vdQsNXP5Qg+0depSxyvuzaavaMaZNynzB4r0KKxX
d9W8qNFcWb/7BWFYgmw7TmJjIn0F/6pKrG75MUrj5Jc6cQMRfNuJrSjEYpsBkG2e
LWy5QBTboDtNnldB6vMR8X25ja25UqiMuvP1HY4OGPX3hYvDVX2IP67BY7i/hb/9
3SwQYWjH38lfSdHlC14FcOWVzWkICm+rEgUKolNy37Rw0wIDAQABo1AwTjAdBgNV
HQ4EFgQUbkS8taBrhQDq7t19qFfRzi8q86kwHwYDVR0jBBgwFoAUbkS8taBrhQDq
7t19qFfRzi8q86kwDAYDVR0TBAUwAwEB/zANBgkqhkiG9w0BAQsFAAOCAQEAD8JW
PJrqtwaTtmmpFv8Xn8K2Tq7BBKg7ANtEs9Ca2SstR9J0idH8YYq69+CHbihO0cVS
QYgkos9FA7NU8eV8twNBBrgSS30ZkIVRCZn72476lHQTWnctqHTqkNmypt5Bdosr
yC9+dy8UCm9UhjW100vu0Oi++/7LU3GOcEuFX65pz4cjFFRLCKmA0mvSiBV4UwWu
HYDzyrZMYYcIpPBj9S7gvoQDeHrpw7yfA5Of+60cZZjwPY9Ebud5ETWnkFqqcShE
PVTB987Vm6hLu5/oHF+JVW05ZdXID1BZvukBYXnwY9OHvU7fin8N/eT/SBTd2HxO
d1SSYr2U5pj0tNqaDQ==
-----END CERTIFICATE-----\0";

// This is PEM_SELF_SIGNED_CERT with a change in the second-to-last
// byte of the signature from 0x9a to 0x9b.
pub const PEM_SELF_SIGNED_CERT_INVALID_SIG: &'static [u8] = b"-----BEGIN CERTIFICATE-----
MIIDCTCCAfGgAwIBAgIJALWh9vlifeRuMA0GCSqGSIb3DQEBCwUAMBoxGDAWBgNV
BAMTD21iZWR0bHMuZXhhbXBsZTAgFw0xODExMjMwNTQ5MTBaGA8yMTAwMDEwMTA1
NDkxMFowGjEYMBYGA1UEAxMPbWJlZHRscy5leGFtcGxlMIIBIjANBgkqhkiG9w0B
AQEFAAOCAQ8AMIIBCgKCAQEAxYwIJgiVJigEPzgINDAYdxNvpeWrEh3QTZk5tIK9
75p5hXFKpSKVBtwRnfOaNHPV+ap8QSiWn0yS7tsUao8dUzJQXbVaT9Al8uaj2MLz
vFFiBsq7J4svBn6Q41xpFBW5vdQsNXP5Qg+0depSxyvuzaavaMaZNynzB4r0KKxX
d9W8qNFcWb/7BWFYgmw7TmJjIn0F/6pKrG75MUrj5Jc6cQMRfNuJrSjEYpsBkG2e
LWy5QBTboDtNnldB6vMR8X25ja25UqiMuvP1HY4OGPX3hYvDVX2IP67BY7i/hb/9
3SwQYWjH38lfSdHlC14FcOWVzWkICm+rEgUKolNy37Rw0wIDAQABo1AwTjAdBgNV
HQ4EFgQUbkS8taBrhQDq7t19qFfRzi8q86kwHwYDVR0jBBgwFoAUbkS8taBrhQDq
7t19qFfRzi8q86kwDAYDVR0TBAUwAwEB/zANBgkqhkiG9w0BAQsFAAOCAQEAD8JW
PJrqtwaTtmmpFv8Xn8K2Tq7BBKg7ANtEs9Ca2SstR9J0idH8YYq69+CHbihO0cVS
QYgkos9FA7NU8eV8twNBBrgSS30ZkIVRCZn72476lHQTWnctqHTqkNmypt5Bdosr
yC9+dy8UCm9UhjW100vu0Oi++/7LU3GOcEuFX65pz4cjFFRLCKmA0mvSiBV4UwWu
HYDzyrZMYYcIpPBj9S7gvoQDeHrpw7yfA5Of+60cZZjwPY9Ebud5ETWnkFqqcShE
PVTB987Vm6hLu5/oHF+JVW05ZdXID1BZvukBYXnwY9OHvU7fin8N/eT/SBTd2HxO
d1SSYr2U5pj0tNqbDQ==
-----END CERTIFICATE-----\0";

pub const PEM_KEY: &'static str = concat!(include_str!("./keys/user.key"),"\0");
pub const PEM_CERT_SUBJECT: &'static str = "CN=mbedtls.example";
pub const PEM_CERT: &'static str = concat!(include_str!("./keys/user.crt"),"\0");

pub const ROOT_CA_CERT_SUBJECT: &'static str = "CN=RootCA";
pub const ROOT_CA_CERT: &'static str = concat!(include_str!("./keys/ca.crt"),"\0");
pub const ROOT_CA_KEY: &'static str = concat!(include_str!("./keys/ca.key"),"\0");

pub const EXPIRED_CERT_SUBJECT: &'static str = "CN=ExpiredNode";
pub const EXPIRED_CERT: &'static str = concat!(include_str!("./keys/expired.crt"),"\0");
pub const EXPIRED_KEY: &'static str = concat!(include_str!("./keys/expired.key"),"\0");

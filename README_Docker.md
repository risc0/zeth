# Raiko Docker tutorial

This tutorial was created to help you set up Raiko using a Docker container.

Raiko leverages [Intel SGX][sgx] through [Gramine][gramine]. Since Gramine supports only [a few distributions][gramine-distros] including Ubuntu, the Docker image is based on Ubuntu.

[gramine-distros]: https://github.com/gramineproject/gramine/discussions/1555#discussioncomment-7016800
[gramine]: https://gramineproject.io/

## Prerequisites

### SGX-enabled CPU

Ensure your machine has an [SGX][sgx]-enabled CPU to run raiko. You can check if your CPU supports SGX (Software Guard Extensions) on Linux by using the [`cpuid`][cpuid] tool.

1.  Install `cpuid` if it's not already installed. On Ubuntu, you can do this with the following command:

    sudo apt-get install cpuid

1.  Run `cpuid` and `grep` for SGX:

        cpuid | grep -i sgx

    If your CPU supports SGX, you should see output similar to this:

    ```
    SGX: Software Guard Extensions supported = true
    ```

    If you don't see this line, your CPU does not support SGX.

Alternatively, you can run `grep sgx /proc/cpuinfo`. If the command returns no output, your CPU doesn't support SGX.

[sgx]: https://www.intel.com/content/www/us/en/architecture-and-technology/software-guard-extensions.html
[cpuid]: https://manpages.ubuntu.com/manpages/noble/en/man1/cpuid.1.html

### Modern Linux kernel

Starting with Linux kernel version [`5.11`][kernel-5.11], the kernel provides out-of-the-box support for SGX. However, it doesn't support [EDMM][edmm] (Enclave Dynamic Memory Management), which Raiko requires. EDMM support first appeared in Linux `6.0`, so ensure that you have Linux kernel `6.0` or above.

To check version of your kernel run:

```
uname -a
```

If you are using Ubuntu and you want to find what are the available Linux kernel versions, run:

```
apt search linux-image
```

[kernel-5.11]: https://www.intel.com/content/www/us/en/developer/tools/software-guard-extensions/linux-overview.html
[edmm]: https://gramine.readthedocs.io/en/stable/manifest-syntax.html#edmm

## Building Docker image

Taiko doesn't provide prebuilt Docker image (yet). You need to build it yourself.

1. Clone `raiko` repository:
   ```
   git clone git@github.com:taikoxyz/raiko.git
   ```
1. Change active directory:
   ```
   cd raiko/docker
   ```
1. Build the image:
   ```
   docker compose build
   ```
1. That's it! You should now be able to find the `raiko:latest` in the list of all Docker images:
   ```
   docker image ls
   ```

## Running Docker container

After successfully building Docker image, you are now able to bootstrap and run Raiko as a daemon.

### Raiko bootstrapping

Bootstrapping is the process of generating a public-private key pair, which will be used for doing signatures within the SGX enclave. The private key is stored in an [encrypted][gramine-encrypted-files] format in the `~/.config/raiko/secrets/priv.key` file. Encryption and decryption are performed inside the enclave, providing protection against malicious attacks.

1. Make sure you haven't generated Raiko's public-private key pair yet:
   ```
   ls ~/.config/raiko/secrets
   ```
   If you `secrets` directory is not empty, you can skip Raiko bootstrapping.
1. Bootstrap Raiko:
   ```
   docker compose run --rm raiko --init
   ```
   It creates a new, encrypted private key in `~/.config/raiko/secrets` directory. It also prints a public key that you need to send to the Taiko team for registration.
   Register the "Instance address"(pinted by `--init` command) with the Taiko team. Once the Taiko team registers your instance, you will be able to use it to sign proofs.

[gramine-encrypted-files]: https://gramine.readthedocs.io/en/stable/manifest-syntax.html#encrypted-files

### Running Raiko daemon

Once you have Raiko bootstrapped, you can start Raiko daemon.

```
docker compose up raiko -d
```

Start the Raiko daemon. Skip `-d` (which stands for _daemon_) to run in the foreground instead.

### Test Raiko

Now, once you have Raiko up and running, you can test it to make sure it is serving requests as expected.

1. Open new terminal and run:
   ```
   tail -f /var/log/raiko/raiko.log.dd-mm-yyyy
   ```
   to monitor requests that you will be sending. Replace `dd-mm-yyyy` placeholder with the current date.
1. Send a sample request to Raiko:
   ```
   curl --location --request POST 'http://localhost:8080' --header 'Content-Type: application/json' --data-raw '{
     "jsonrpc": "2.0",
     "id": 1,
     "method": "proof",
     "params": [
       {
         "type": "Sgx",
         "l2Rpc": "https://rpc.internal.taiko.xyz",
         "l1Rpc": "https://l1rpc.internal.taiko.xyz",
         "block": 2,
         "prover": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
         "graffiti": "0000000000000000000000000000000000000000000000000000000000000000"
       }
     ]
   }'
   ```
   If the request was served correctly, you should see a lot of logs being produced in the log file and an SGX proof printed on the standard output:
   ```
   {"jsonrpc":"2.0","id":1,"result":{"type":"Sgx","proof":"0x000000006cbe8f8cb4c319f5beba9a4fa66923105dc90aec3c5214eed022323b9200097b647208956cc1b7ce0d8c0777df657caace329cc73f2398b137095128c7717167fc52d6474887e98e0f97149c9be2ca63a458dc8a1b"}}
   ```

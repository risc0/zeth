/* Copyright (c) Fortanix, Inc.
 *
 * Licensed under the GNU General Public License, version 2 <LICENSE-GPL or
 * https://www.gnu.org/licenses/gpl-2.0.html> or the Apache License, Version
 * 2.0 <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0>, at your
 * option. This file may not be copied, modified, or distributed except
 * according to those terms. */

use std::io::{Error as IoError, Result as IoResult};
use std::net::TcpStream;
use std::os::unix::io::FromRawFd;

pub fn create_tcp_pair() -> IoResult<(TcpStream, TcpStream)> {
    let mut fds: [libc::c_int; 2] = [0; 2];
    unsafe {
        // one might consider creating a TcpStream from a UNIX socket a hack
        // most socket operations should work the same way, and UnixSocket
        // is too new to be used
        if libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) == 0 {
            Ok((TcpStream::from_raw_fd(fds[0]), TcpStream::from_raw_fd(fds[1])))
        } else {
            Err(IoError::last_os_error())
        }
    }
}

#[cfg(feature = "tokio")]
pub fn create_tcp_pair_async() -> IoResult<(tokio::net::TcpStream, tokio::net::TcpStream)> {
    let (c, s) = create_tcp_pair()?;
    c.set_nonblocking(true)?;
    s.set_nonblocking(true)?;
    Ok((tokio::net::TcpStream::from_std(c)?, tokio::net::TcpStream::from_std(s)?))
}

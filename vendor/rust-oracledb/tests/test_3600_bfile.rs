//-----------------------------------------------------------------------------
// Copyright (c) 2026, Oracle and/or its affiliates.
//
// This software is dual-licensed to you under the Universal Permissive License
// (UPL) 1.0 as shown at https://oss.oracle.com/licenses/upl and Apache License
// 2.0 as shown at http://www.apache.org/licenses/LICENSE-2.0. You may choose
// either license.
//
// If you elect to accept the software under the Apache License, Version 2.0,
// the following applies:
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------
// test_3600_bfile()
//-----------------------------------------------------------------------------

mod common;

use common::conn;
use rstest::*;
use std::io::{Read, Write};

/// Owns the server files used by one BFILE test in a configured directory.
struct BfileFixture {
    conn: oracledb::Connection,
    directory: String,
    file_prefix: String,
    files: Vec<String>,
}

impl BfileFixture {
    fn fetch(&self, filename: &str) -> Result<oracledb::Lob, oracledb::Error> {
        let filename = format!("{}-{filename}", self.file_prefix);
        let mut row = self.conn.query_row(
            "select bfilename(:1, :2) from dual",
            &[&self.directory, &filename],
        )?;
        row.take(0)
    }

    fn new() -> Option<Self> {
        let Ok(directory) = std::env::var("RSO_TEST_DIRECTORY_NAME") else {
            common::skip_test("requires RSO_TEST_DIRECTORY_NAME");
            return None;
        };

        Some(Self {
            conn: common::conn(),
            directory,
            file_prefix: format!("rso-bfile-{}", uuid::Uuid::new_v4()),
            files: Vec::new(),
        })
    }

    fn write_file(
        &mut self,
        filename: &str,
        contents: &[u8],
    ) -> Result<(), oracledb::Error> {
        let filename = format!("{}-{filename}", self.file_prefix);
        self.files.push(filename.clone());
        self.conn.execute(
            "declare
                file_handle utl_file.file_type;
                contents raw(32767) := :1;
            begin
                file_handle := utl_file.fopen(:2, :3, 'wb', 32767);
                if contents is not null then
                    utl_file.put_raw(file_handle, contents);
                end if;
                utl_file.fclose(file_handle);
            exception
                when others then
                    if utl_file.is_open(file_handle) then
                        utl_file.fclose(file_handle);
                    end if;
                    raise;
            end;",
            &[&contents, &self.directory, &filename],
        )?;
        Ok(())
    }
}

impl Drop for BfileFixture {
    fn drop(&mut self) {
        for filename in &self.files {
            let _ = self.conn.execute(
                "begin utl_file.fremove(:1, :2); end;",
                &[&self.directory, filename],
            );
        }
    }
}

/// Fetching a non-null BFILE returns a read-only LOB with either fetch mode.
#[rstest]
fn test_3600(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let sql =
        "select bfilename('RSO_MISSING_DIRECTORY', 'missing.bin') from dual";
    for fetch_lobs in [false, true] {
        let mut builder = conn.statement(sql)?;
        if fetch_lobs {
            builder = builder.fetch_lobs();
        }
        let statement = builder.build()?;
        let mut cursor = statement.query(&[])?;
        assert_eq!(cursor.columns()[0].db_type(), oracledb::DB_TYPE_BFILE);
        let mut row = cursor.next().unwrap()?;
        let mut lob: oracledb::Lob = row.take(0)?;
        assert_eq!(lob.db_type(), oracledb::DB_TYPE_BFILE);
        assert_eq!(
            lob.write(&[1]).unwrap_err().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert_eq!(
            lob.trim(0).unwrap_err().kind(),
            &oracledb::ErrorKind::OperationNotSupportedOnBfile
        );
    }
    let mut lob = conn.create_lob(oracledb::DB_TYPE_BLOB)?;
    assert_eq!(
        lob.file_exists().unwrap_err().kind(),
        &oracledb::ErrorKind::OperationOnlySupportedOnBfile
    );
    Ok(())
}

/// Fetching a NULL BFILE returns 'None' with either fetch mode.
#[rstest]
fn test_3601(conn: oracledb::Connection) -> Result<(), oracledb::Error> {
    let _guard = common::create_table(&conn, "test_3601", "data bfile")?;
    conn.execute("insert into test_3601 values (null)", &[])?;
    for fetch_lobs in [false, true] {
        let mut builder = conn.statement("select data from test_3601")?;
        if fetch_lobs {
            builder = builder.fetch_lobs();
        }
        let row = builder.build()?.query_row(&[])?;
        let lob: Option<oracledb::Lob> = row.get(0)?;
        assert!(lob.is_none());
    }
    Ok(())
}

/// Tests BFILE operations using a file populated with known data.
#[test]
fn test_3602() -> Result<(), oracledb::Error> {
    let Some(mut fixture) = BfileFixture::new() else {
        return Ok(());
    };
    // Includes every byte value and spans multiple 17-byte reads.
    let contents: Vec<u8> = (0..4099).map(|i| (i % 256) as u8).collect();
    fixture.write_file("data.bin", &contents)?;
    let mut lob = fixture.fetch("data.bin")?;
    assert_eq!(lob.db_type(), oracledb::DB_TYPE_BFILE);
    assert!(lob.file_exists()?);
    assert!(!lob.is_open()?);

    assert_eq!(lob.get_size()?, contents.len());

    lob.open()?;
    assert!(lob.is_open()?);
    let mut data = Vec::new();
    let mut buffer = [0; 17];
    loop {
        let count = lob.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..count]);
    }
    assert!(lob.is_open()?);
    lob.close()?;
    assert!(!lob.is_open()?);
    assert_eq!(data, contents);

    let mut auto_open_lob = fixture.fetch("data.bin")?;
    let mut auto_open_data = Vec::new();
    auto_open_lob.read_to_end(&mut auto_open_data)?;
    assert_eq!(auto_open_data, contents);
    assert!(!auto_open_lob.is_open()?);
    Ok(())
}

/// An empty server file exists and reads as an empty byte stream.
#[test]
fn test_3603() -> Result<(), oracledb::Error> {
    let Some(mut fixture) = BfileFixture::new() else {
        return Ok(());
    };
    fixture.write_file("empty.bin", &[])?;
    let mut lob = fixture.fetch("empty.bin")?;
    assert!(lob.file_exists()?);
    assert_eq!(lob.get_size()?, 0);
    let mut data = Vec::new();
    lob.read_to_end(&mut data)?;
    assert!(data.is_empty());
    assert!(!lob.is_open()?);
    Ok(())
}

/// A missing file in a valid server directory does not exist and cannot be read.
#[test]
fn test_3604() -> Result<(), oracledb::Error> {
    let Some(fixture) = BfileFixture::new() else {
        return Ok(());
    };
    let mut lob = fixture.fetch("missing.bin")?;
    assert!(!lob.file_exists()?);
    assert!(lob.read(&mut [0; 17]).is_err());
    Ok(())
}

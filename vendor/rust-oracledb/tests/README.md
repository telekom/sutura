# Test rust-oracledb

This software is dual-licensed to you under the Universal Permissive License
(UPL) 1.0 as shown at https://oss.oracle.com/licenses/upl and Apache License
2.0 as shown at https://www.apache.org/licenses/LICENSE-2.0. You may choose
either license.

If you elect to accept the software under the Apache License, Version 2.0,
the following applies:

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

   https://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.


## Run Tests

Set the following environment variables to provide credentials for the test
suite.

* `RSO_TEST_MAIN_USER` provides the name of the user which will be used for
  running the test suite.

* `RSO_TEST_MAIN_PASSWORD` provides the password of the user which will be used
  for running the test suite.

* `RSO_TEST_CONNECT_STRING` provides the connection string that points to your
  database's location.

* `RSO_TEST_ADMIN_USER` provides the name of the DBA user which will be used
  for running the test suite.

* `RSO_TEST_ADMIN_PASSWORD` provides the password of the DBA user will be used
  for running the test suite.

* `RSO_TEST_DIRECTORY_NAME` specifies an existing DIRECTORY object for tests
  that create server files. The referenced path must exist and be writable by
  the OS user running the database.

Once these environment variables have been set, run the script for creating the
test user:

```
cargo run --bin create_test_schema
```

This will drop the test user if it exists, then create the test user and ensure
all necessary grants have been granted.

To run the complete test suite,

```
cargo test --all-features
```

To run the complete test suite and ensure that the skipped test messages are displayed,

```
cargo test --all-features -- --nocapture
```

To run a specific test file,

```
cargo test --test <test_file>
```
Do not use the .rs extension here.

# giant-squid

This document explains the design choices I made while designing the solution to
the coding test.


## Usage

### Building
Simply run `cargo build` to run the engine on the data, which is included
in this repo to provide you with a "batteries included" experience.

### Running
The code can be run as specified in the PDF:
`cargo run -- transactions.csv > accounts.csv`

As indicated, the output of the execution is printed to `stdout`.

### Testing
The project's built-in tests can be run using `cargo test`.


## Design decisions

## General design

The design of the processor is pretty straightforward: It reads in a CSV file
asynchronously using a transaction stream, and then pipelines the streamed
transactions asynchronously to `Transactor::process_transaction()`, which
operates on a single transaction and delegates the actual processing to a
sibling method depending on the `TransactionType` of the transaction.

The Transactor acts as an accumulator of sorts. It keeps track of the open
accounts, and applies transactions it considers valid to those accounts.
Transactions that are found invalid for any reason are ignored.

After all the transactions are processed, the program uses the async
`crate::main::print_output()` fn to print the desired output.

### Dependencies

#### Replace `csv` with `csv-async`
I opted to use `csv-async` rather than `csv` to read in CSV files.
This is because, as the name suggests, `csv-async` is meant to work
in async contexts, whereas `csv` was never designed for that purpose.
In particular, `csv-async` produces an async stream.
Otherwise it provides almost the exact same API as the `csv` crate.

#### `rust_decimal` for decimal manipulation
Operations on native floating point types, while fast due to direct hardware
support, can be notoriously inaccurate, and in financial settings of course
accuracy matters.  Therefore my choice here is `rust_decimal` to manipulate
financial amounts, at the expense of some performance.

### Error Handling

Where possible, errors defined in this crate are fallible. Panicking is
generally a quick-and-dirty solution, but for production purposes fallible
errors are highly preferable, even when the end result is that the process is
aborted anyway. This is because fallible errors provide the choice of what to
do with them.  That is, they don't enforce policy and only provide mechanism.

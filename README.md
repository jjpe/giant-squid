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


## Scoring

### Basics
1. Does the app build? *yes*
2. Does it read and write data in the way we'd like it to? *yes*
3. Is it properly formatted? *yes*

### Completeness
1. Do you handle all of the cases, including disputes, resolutions, and
chargebacks? *yes, though I am not 100% certain of the chargeback case.
Something about the spec seems a bit off.*
2. Maybe you don't handle disputes and resolutions but you
can tell when a transaction is charged back. Try to cover as much as you
can *see 1.*

### Correctness
1. For the cases you are handling are you handling them correctly? *I think so*
2. How do you know this? *By unit testing the Transactor*
3. Did you test against sample data? *yes*
4. If so, include it in the repo. *The sample data is incorporated in the tests.*
5. Did you write unit tests for the complicated bits? *yes*
6. Or are you using the type system to ensure correctness?
    *To a degree, e.g. by newtyping ClientId and TransactionId.
    But given the time restriction, I opted not to use a typestate
    construction, as I wasn't sure how that would interact with async
    code and so I coulnd't be sure it wouldn't be a huge time sink.*

### Safety and Robustness
1. Are you doing something dangerous? *No*
2. Tell us why you chose to do it this way.
    *I actively avoid constructs like `unsafe` when I can.*
3. How are you handling errors?
    *By defining fallible error enums, and `match`ing on variant values*

### Efficiency
1. Be thoughtful about how you use system resources. Sample data sets may
be small but you may be evaluated against much larger data sets (hint:
transaction IDs are valid u32 values). Can you stream values through
memory as opposed to loading the entire data set upfront?
    *Assuming my mental model of async Rust code is correct: yes*
2. What if your code was bundled in a server, and these CSVs came from
    thousands of concurrent TCP streams?
    *The Transactor would need a new method, that consumes a transaction
    stream and processes the transactions much as it does now in
    `Transactor::process_csv_file()`.* The Transactor, while async, is not
    necessarily multithreaded. However, in principle multiple versions of
    the server could be run.

### Maintainability
In this case clean code is more important than efficient code because
humans will have to read and review your code without an opportunity for
you to explain it. Inefficient code can often be improved if it is correct
and highly maintainable.
*I wrote the code to be just about as maintainable as I could.*

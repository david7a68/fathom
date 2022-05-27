# Architecture

## Goals

- To support the use of a microservice architecture, be it now or in the future.
- To clearly delineate units of functionality, and provide the framework within
  which they can be tested.
- To minimize impedance mismatch between the developer's intent and the code's
  structure.

## Constraints

- The resulting architecture must be simple to understand and work with, and
  simple to deploy.
- The entire system should be permit automated end-to-end testing in a variety
  of simulated conditions.

## Assorted notes

- is the gateway special?
  - dispatches gRPC calls to individual services
  - must be able to add and remove services
  - return Unavailable if no services of particular kind available
  - load balances between instances of each service?
  - both client and server
    - is there any way to unify static/remote?
- need to be able to have services be empty?
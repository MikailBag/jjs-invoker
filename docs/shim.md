# Shim
Shim is special entity that preprocesses all invoke requests invoker receives.

This repository defines standard shim that uses Docker images to distribute files.
## Protocol
Shim must implement web server (address is passed to the invoker as a `--shim` parameter).
This server must define endpoint `POST /on-request`. Request and response format are specified further.

Please note that shim does not have to implement any authentication.
### Request
Request body contains the invoke request itself. Please note that in the shim mode invoker does not
validate incoming request to be valid `InvokeRequest`. That way shim can provide extend invoke requests with new fields.
If request is similar of InvokeRequest, shim can make use of `ext` fields and reuse InvokeRequest.
### Response
#### Accept and modfiy
If the shim successfully preprocessed request, it should respond with code `200`.
Body must be a map and contain `result` key. Value of this key must be valid `InvokeRequest`
object and it will be then executed by the invoker.
#### Reject
If the shim wishes to reject the request, it should respond with code `400`.
Body must be a map and contain `error` key. Value of this key can be arbitrary and it will
be returned to the user as is.

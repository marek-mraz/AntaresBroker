*** Settings ***
Documentation       Check that a Context Source Registration's contextSourceInfo pairs are
...                 HTTP headers (CIM 009 clause 6.3.19): "Key and value members shall adhere
...                 to IETF RFC 7230 Hypertext Transfer Protocol (HTTP/1.1): Message Syntax
...                 and Routing definitions concerning HTTP headers". A pair that no HTTP
...                 message can carry is refused as the registration content it is
...                 (clause 5.9.2.4 BadRequestData), not accepted and left to fail at the
...                 first forward.
...
...                 Antares extension TP — the official TPs do not exercise the RFC 7230
...                 constraint of clause 6.3.19.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
033_13_01 A Token Key And A Visible Value Are Accepted
    [Documentation]    control: an RFC 7230 field-name and field-value stay registrable
    [Tags]    csr-create    6_3_19    since_v1.9.1
    Register With Header Pair Expecting    201    X-Auth-Token    Bearer abc.def-ghi_jkl

033_13_02 A Key Carrying CRLF Is Rejected
    [Documentation]    a field-name is a token: CR and LF would split the forwarded request
    [Tags]    csr-create    6_3_19    since_v1.9.1
    ${key}=    Evaluate    "X-Injected" + chr(13) + chr(10) + "X-Second"
    Register With Header Pair Expecting    400    ${key}    v

033_13_03 A Key Carrying A Space Is Rejected
    [Tags]    csr-create    6_3_19    since_v1.9.1
    Register With Header Pair Expecting    400    X Injected    v

033_13_04 A Key Carrying A Colon Is Rejected
    [Documentation]    ":" separates a field-name from its value and is not a tchar
    [Tags]    csr-create    6_3_19    since_v1.9.1
    Register With Header Pair Expecting    400    X:Injected    v

033_13_05 An Empty Key Is Rejected
    [Documentation]    a field-name is 1*tchar
    [Tags]    csr-create    6_3_19    since_v1.9.1
    Register With Header Pair Expecting    400    ${EMPTY}    v

033_13_06 A Value Carrying CRLF Is Rejected
    [Documentation]    a field-value carries no CR or LF
    [Tags]    csr-create    6_3_19    since_v1.9.1
    ${value}=    Evaluate    "a" + chr(13) + chr(10) + "X-Injected: 1"
    Register With Header Pair Expecting    400    X-Custom    ${value}

033_13_07 A Value Carrying A NUL Is Rejected
    [Tags]    csr-create    6_3_19    since_v1.9.1
    ${value}=    Evaluate    "a" + chr(0) + "b"
    Register With Header Pair Expecting    400    X-Custom    ${value}


*** Keywords ***
Register With Header Pair Expecting
    [Arguments]    ${expected_status_code}    ${key}    ${value}
    ${id}=    Generate Random CSR Id
    ${body}=    Evaluate
    ...    json.dumps({"id": $id, "type": "ContextSourceRegistration", "endpoint": "http://peer.example/ngsi-ld/v1", "information": [{"entities": [{"type": "Building"}]}], "contextSourceInfo": [{"key": $key, "value": $value}]})
    ...    modules=json
    ${response}=    POST
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}
    ...    data=${body}
    ...    headers=${{ {"Content-Type": "application/json"} }}
    ...    expected_status=any
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    ${expected_status_code} == 201
        Delete Context Source Registration    ${id}
    END

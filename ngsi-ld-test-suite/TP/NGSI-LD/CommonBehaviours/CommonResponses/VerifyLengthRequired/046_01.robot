*** Settings ***
Documentation       Verify that a POST, PATCH or PUT request which does not define the
...                 Content-Length HTTP header is answered with a bare 411.
...
...                 Mandated twice and unconditionally:
...                 - 6.3.2: "\"Length required\" (411) which shall be raised when an HTTP
...                 request provided by a client does not define the \"Content-Length\"
...                 HTTP header."
...                 - 6.3.4: "For HTTP POST, PATCH and PUT HTTP requests implementations
...                 shall check the following preconditions: … Content-Length header shall
...                 include the length of the request payload body" → "\"Content-Length\"
...                 HTTP header absence, shall result in just a 411 HTTP status code
...                 (without any payload body)."
...
...                 No exemption is granted for Transfer-Encoding: chunked, which is how
...                 these cases omit the header (an ordinary requests call always sets it).
...
...                 Antares extension TP — the suite has NO 411 coverage at all, although
...                 its two siblings from the same clause do (415 in 048_01, 406 in
...                 049_01/049_02). That gap is why the requirement went unimplemented.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Variables ***
${entity_body}      {"id": "urn:ngsi-ld:Building:411-01", "type": "Building"}


*** Test Cases ***
046_01_01 POST Without Content-Length
    [Documentation]    5.6.1 Create Entity with no Content-Length → bare 411
    [Tags]    common-behaviours    6_3_2    6_3_4    since_v1.9.1

    ${response}=    Send Without Content Length    POST    ${url}/${ENTITIES_ENDPOINT_PATH}
    Bare 411    ${response}

046_01_02 PATCH Without Content-Length
    [Documentation]    5.6.2 Update Attributes with no Content-Length → bare 411
    [Tags]    common-behaviours    6_3_2    6_3_4    since_v1.9.1

    ${response}=    Send Without Content Length
    ...    PATCH
    ...    ${url}/${ENTITIES_ENDPOINT_PATH}urn:ngsi-ld:Building:411-01/attrs/
    Bare 411    ${response}

046_01_03 PUT Without Content-Length
    [Documentation]    5.6.18 Replace Entity with no Content-Length → bare 411
    [Tags]    common-behaviours    6_3_2    6_3_4    since_v1.9.1

    ${response}=    Send Without Content Length
    ...    PUT
    ...    ${url}/${ENTITIES_ENDPOINT_PATH}urn:ngsi-ld:Building:411-01
    Bare 411    ${response}

046_01_04 GET Is Outside The Precondition
    [Documentation]    6.3.4 scopes the Content-Length precondition to POST, PATCH and PUT
    ...    — a GET must not be refused for lacking it
    [Tags]    common-behaviours    6_3_2    6_3_4    since_v1.9.1

    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    params=type=Building
    ...    expected_status=any
    Should Not Be Equal As Integers    ${response.status_code}    411


046_01_05 The Precondition Is Decided Before The Tenant Is Looked Up
    [Documentation]    6.3.4 grants the missing Content-Length no exemption, so naming a
    ...    Tenant that does not exist cannot turn the bare 411 into a 404 with a body —
    ...    the precondition holds whatever the request targets
    [Tags]    common-behaviours    6_3_2    6_3_4    4_14    since_v1.9.1

    ${response}=    Send Without Content Length
    ...    POST
    ...    ${url}/${ENTITIES_ENDPOINT_PATH}
    ...    ${{ {"NGSILD-Tenant": "tp46105nosuchtenant"} }}
    Bare 411    ${response}
    Should Not Be Equal As Integers    ${response.status_code}    404


*** Keywords ***
Send Without Content Length
    [Documentation]    requests sets Content-Length for any ordinary body, so the body is
    ...    passed as an iterator — that selects chunked transfer, which carries no
    ...    Content-Length. 6.3.4 grants chunked no exemption.
    [Arguments]    ${method}    ${target}    ${extra_headers}=${{ {} }}
    ${response}=    Evaluate
    ...    __import__("requests").request($method, $target, data=iter([$entity_body.encode()]), headers={"Content-Type": "application/json", **$extra_headers}, timeout=20)
    RETURN    ${response}

Bare 411
    [Arguments]    ${response}
    Check Response Status Code    411    ${response.status_code}
    Should Be Empty    ${response.text}    411 shall carry no payload body

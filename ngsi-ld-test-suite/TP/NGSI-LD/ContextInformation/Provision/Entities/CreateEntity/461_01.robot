*** Settings ***
Documentation       Verify 4.6.1 Supported text encodings.
...
...                 4.6.1: "NGSI-LD implementations shall support the UTF-8 text
...                 encoding format. ... applications shall provide JSON content
...                 encoded using UTF-8 and NGSI-LD systems shall also expose such
...                 JSON content using UTF-8."
...
...                 Antares extension TP — the official suite has zero non-ASCII
...                 payload coverage. The non-UTF-8-body error path (InvalidRequest
...                 400) is unit-tested broker-side; the suite's JSON keywords
...                 cannot emit invalid byte sequences.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
461_01_01 UTF-8 Content Round-Trips Byte-Exact
    [Documentation]    4.6.1: UTF-8 JSON accepted on input and exposed unaltered on
    ...    output — multibyte content (diacritics, dash, euro sign) must round-trip
    ...    exactly and the response must NOT contain the U+FFFD replacement
    ...    character (mojibake).
    [Tags]    e-create    e-retrieve    4_6_1    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "brandName": {"type": "Property", "value": "žltý kôň — 100 €"}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${value}=    Evaluate    $response.json()['brandName']['value']
    Should Be Equal    ${value}    žltý kôň — 100 €
    ${raw}=    Evaluate    $response.content.decode('utf-8')
    Should Not Contain    ${raw}    �
    [Teardown]    Delete Entity    ${entity_id}

*** Settings ***
Documentation       Verify 4.6.4 Supported Content.
...
...                 4.6.4: incoming values may contain < > " ' = ; ( ). "In all
...                 cases, implementations shall preserve the representation of
...                 the content of the values provided by the context information
...                 providers and return the original content when replying to
...                 context consumption requests."
...
...                 Antares extension TP — no official TP sends script-injection
...                 characters in a Property value.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
464_01_01 Script-Injection Characters Round-Trip Verbatim
    [Documentation]    4.6.4: a value holding < > " ' = ; ( ) is accepted and the
    ...    ORIGINAL content is returned — the response must NOT contain an
    ...    HTML-escaped (&lt;) or unicode-escaped (\\u003c) rendering.
    [Tags]    e-create    e-retrieve    4_6_4    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload_value}=    Set Variable    <script>alert('x')</script> "q" = ; ( )
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "note": {"type": "Property", "value": $payload_value}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${value}=    Evaluate    $response.json()['note']['value']
    Should Be Equal    ${value}    ${payload_value}
    Should Not Contain    ${response.text}    &lt;
    Should Not Contain    ${response.text}    \\u003c
    [Teardown]    Delete Entity    ${entity_id}

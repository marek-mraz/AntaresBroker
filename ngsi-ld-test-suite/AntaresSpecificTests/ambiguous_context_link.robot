*** Settings ***
Documentation       Verify that two Link headers naming DIFFERENT @context
...                 documents are refused, while the same document twice is not.
...
...                 ANTARES DECISION, NOT A CIM 009 REQUIREMENT. 6.3.5 says the
...                 @context "shall be obtained from a Link Header as mandated by
...                 JSON-LD [2], section 6.2", and that clause raises a multiple
...                 context link headers error rather than choosing between them.
...                 CIM 009 itself says nothing: its only statement is Annex C.8,
...                 which is informative and prescribes a wrapper @context rather
...                 than an error. Antares refuses with BadRequestData 400.
...
...                 The reason is what the @context decides. It is not one filter
...                 among many: it fixes what EVERY term in the request means, so
...                 serving a request against one of two links stores it under an
...                 expansion the client never designated. The same gateway seam
...                 as the repeated query parameter applies with more force — the
...                 policy layer in front of the broker can read the other link.
...
...                 Antares is narrower than JSON-LD on purpose: the SAME target
...                 twice is accepted, because an intermediary may repeat a field
...                 line verbatim and there is no ambiguity in a repeat that
...                 names one document.

Library             RequestsLibrary
Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Variables ***
${REL}                  rel="http://www.w3.org/ns/json-ld#context"
${CORE_CONTEXT_URL}     https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld
${OTHER_CONTEXT_URL}    https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.7.jsonld


*** Test Cases ***
Two Link Headers Naming Different Contexts Are Refused
    [Documentation]    Both spellings RFC 8288 allows for two link-values name
    ...    two @context documents, and neither names one the broker may pick.
    [Tags]    antares-specific    jsonld-context    context-ambiguity
    ${a}=    Set Variable    <${CORE_CONTEXT_URL}>; ${REL}
    ${b}=    Set Variable    <${OTHER_CONTEXT_URL}>; ${REL}
    Context Link Should Be Refused    ${a}, ${b}
    # order does not make one of them the answer
    Context Link Should Be Refused    ${b}, ${a}

The Same Context Twice Is Not Ambiguous
    [Documentation]    A field line an intermediary repeated names one document,
    ...    so the request is answered rather than refused.
    [Tags]    antares-specific    jsonld-context
    ${a}=    Set Variable    <${CORE_CONTEXT_URL}>; ${REL}
    Context Link Should Be Accepted    ${a}, ${a}

A Second Link With Another Relation Changes Nothing
    [Documentation]    Only the JSON-LD context relation designates an @context;
    ...    a link with any other relation is a different link entirely.
    [Tags]    antares-specific    jsonld-context
    ${a}=    Set Variable    <${CORE_CONTEXT_URL}>; ${REL}
    Context Link Should Be Accepted    ${a}, <${OTHER_CONTEXT_URL}>; rel="alternate"
    Context Link Should Be Accepted    <${OTHER_CONTEXT_URL}>; rel="self", ${a}


*** Keywords ***
Context Link Should Be Refused
    [Arguments]    ${link}
    ${headers}=    Create Dictionary    Link=${link}
    ${response}=    GET    url=${url}/entities?type=Vehicle    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Should Be Equal As Strings    ${response.json()}[type]
    ...    https://uri.etsi.org/ngsi-ld/errors/BadRequestData

Context Link Should Be Accepted
    [Arguments]    ${link}
    ${headers}=    Create Dictionary    Link=${link}
    ${response}=    GET    url=${url}/entities?type=Vehicle    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}

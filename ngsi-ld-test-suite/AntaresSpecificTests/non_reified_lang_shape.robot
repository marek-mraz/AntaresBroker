*** Settings ***
Documentation       Verify that a client-supplied non-reified `lang` member is a
...                 language tag string rather than arbitrary JSON.
...
...                 ANTARES DECISION, NOT A CIM 009 REQUIREMENT. Clause 4.15
...                 makes `lang` a member the BROKER produces: when the language
...                 filter converts a LanguageProperty into a Property, "the
...                 attribute in question shall be augmented with an additional
...                 non-reified subproperty lang indicating the actual language
...                 returned". No clause says what happens when a Context
...                 Producer supplies one, and no member table lists `lang` as
...                 an input member of a Property, so the spelling of the rule
...                 is Antares': the member is stored, and it is stored as a
...                 string.
...
...                 The reason is the reader on the other side. `lang` is
...                 non-reified, so its value IS the language tag — a consumer
...                 written against 4.15 reads it as one. Stored as an object or
...                 an array it becomes a shape no such consumer can interpret,
...                 in a member the same consumer cannot tell apart from one the
...                 broker produced. Every other non-reified member of an
...                 instance (unitCode, valueType, datasetId, observedAt) is
...                 checked on the way in; this one was copied through whatever
...                 its JSON type.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource


*** Test Cases ***
A Non String lang Member Is Refused
    [Documentation]    A langtag is a string (RFC 5646); the other JSON types
    ...    leave the instance unreadable to a 4.15 consumer.
    [Tags]    antares-specific    language-filter    non-reified
    [Template]    Create Entity Whose Property Carries lang
    {"en": "hello"}    400
    \["fr"]    400
    7    400
    true    400

A Language Tag String Is Stored And Returned
    [Documentation]    The member is kept — the decision is about its shape, not
    ...    about refusing the member.
    [Tags]    antares-specific    language-filter    non-reified
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "street": {"type": "Property", "value": "Grand Place", "lang": "fr"}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${read}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${read.status_code}
    ${body}=    Set Variable    ${read.json()}
    Should Be Equal    ${body}[street][lang]    fr
    [Teardown]    Delete Entity    ${entity_id}


*** Keywords ***
Create Entity Whose Property Carries lang
    [Documentation]    Plants the member on an ordinary Property, which is the
    ...    shape 4.15 produces, and expects BadRequestData.
    [Arguments]    ${lang_json}    ${expected}
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${lang}=    Evaluate    json.loads('''${lang_json}''')    json
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "street": {"type": "Property", "value": "Grand Place", "lang": $lang}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    ${expected}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

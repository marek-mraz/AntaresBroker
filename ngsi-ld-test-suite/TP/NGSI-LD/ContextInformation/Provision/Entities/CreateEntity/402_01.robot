*** Settings ***
Documentation       Verify the 4.2.2 Meta Model member requirements on entity creation.
...
...                 Clause 4.2.2: "An NGSI-LD Property shall have a value, stated through
...                 hasValue. An NGSI-LD Relationship shall have an object stated through
...                 hasObject." and "An NGSI-LD Value shall be either a rdfs:Literal or a
...                 node object" — a bare JSON null is neither (the NGSI-LD null is the
...                 string sentinel, clause 4.5.2). A typed attribute missing its required
...                 member is invalid content (clause 4.6.4) and shall be rejected with
...                 BadRequestData.
...
...                 Antares extension TP — the official 001_02 invalid-create scenarios
...                 cover malformed JSON and @context negotiation (6.3.5), not the
...                 meta-model member requirements.

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Create Entity Violating The Meta Model


*** Test Cases ***    FILENAME
402_01_01 Property Without Value
    [Tags]    e-create    4_2_2    since_v1.9.1
    meta-model-property-no-value.jsonld
402_01_02 Relationship Without Object
    [Tags]    e-create    4_2_2    since_v1.9.1
    meta-model-relationship-no-object.jsonld
402_01_03 Property With Bare JSON Null Value
    [Tags]    e-create    4_2_2    4_5_2    since_v1.9.1
    meta-model-property-null-value.jsonld
402_01_04 LanguageProperty Without LanguageMap
    [Tags]    e-create    4_2_2    4_5_18    since_v1.9.1
    meta-model-languageproperty-no-map.jsonld


*** Keywords ***
Create Entity Violating The Meta Model
    [Documentation]    4.2.2: the attribute's required member is absent or its value is
    ...    outside the NGSI-LD Value space — creation shall fail with BadRequestData
    [Arguments]    ${filename}
    ${response}=    Create Entity From File    ${filename}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}

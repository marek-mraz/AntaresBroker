*** Settings ***
Documentation       Check that one can append a LanguageProperty property to an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity
Test Template       Append Attributes Without Params


*** Variables ***
${filename}=    vehicle-speed-two-datasetid.jsonld


*** Test Cases ***    STATUS_CODE    FRAGMENT_FILENAME    EXPECTATION_FILENAME
010_06_01 Append A LanguageProperty Property
    [Tags]    ea-append    5_6_3    4_5_18    since_v1.4.1
    204    vehicle-new-language-property-fragment.jsonld    vehicle-language-property-appended.jsonld


*** Keywords ***
Append Attributes Without Params
    [Documentation]    Check that one can append a LanguageProperty property to an entity
    [Arguments]    ${status_code}    ${fragment_filename}    ${expectation_filename}
    ${response}=    Append Entity Attributes    ${entity_id}    ${fragment_filename}    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    context=${ngsild_test_suite_context}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Updated Resource Set To    ${entity_expectation_payload}    ${response1.json()}

Create Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${create_response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${entity_id}

Delete Initial Entity
    Delete Entity    ${entity_id}

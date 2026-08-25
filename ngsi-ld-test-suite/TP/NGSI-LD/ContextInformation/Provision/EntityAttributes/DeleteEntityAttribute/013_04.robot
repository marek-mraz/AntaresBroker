*** Settings ***
Documentation       Check that one can delete a scope from an entity

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity


*** Variables ***
${filename}=                building-minimal-with-one-scope.json
${expectation_filename}=    building-minimal-compacted.json


*** Test Cases ***
013_04 Delete Scope From An Entity
    [Documentation]    Check that one can delete a scope from an entity
    [Tags]    ea-delete    5_6_5    4_18    since_v1.5.1
    ${response}=    Delete Entity Attributes
    ...    ${entity_id}
    ...    scope
    ...    ${EMPTY}
    ...    false
    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    Check Updated Resource Set To    ${entity_expectation_payload}    ${response.json()}


*** Keywords ***
Create Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entity
    Delete Entity    ${entity_id}

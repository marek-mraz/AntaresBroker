*** Settings ***
Documentation       Check that you can delete a VocabProperty property from an entity

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity


*** Variables ***
${filename}=                building-vocab-property-string.jsonld
${expectation_filename}=    building-minimal.json


*** Test Cases ***
013_07_01 Delete A VocabProperty Property From An Entity
    [Documentation]    Check that you can delete a VocabProperty property from an entity
    [Tags]    ea-delete    5_6_5    4_5_20    since_v1.7.1
    ${response}=    Delete Entity Attributes
    ...    ${entity_id}
    ...    vocabProperty
    ...    ${EMPTY}
    ...    false
    Check Response Status Code    204    ${response.status_code}
    ${entity_expectation_payload}=    Load Test Sample    entities/expectations/${expectation_filename}    ${entity_id}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    Check Updated Resource Set To    ${entity_expectation_payload}    ${response.json()}


*** Keywords ***
Create Initial Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Test Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Initial Entity
    Delete Entity    ${entity_id}

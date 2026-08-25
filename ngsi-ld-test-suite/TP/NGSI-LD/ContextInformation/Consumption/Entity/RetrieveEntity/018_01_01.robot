*** Settings ***
Documentation       Check that one can get an entity by id

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Entity
Suite Teardown      Delete Created Entity


*** Variables ***
${filename}=                building-simple-attributes.jsonld
${expectation_filename}=    building-simple-attributes.jsonld


*** Test Cases ***
018_01_01 Get An Entity By Id
    [Documentation]    Check that one can get an entity by id
    [Tags]    e-retrieve    5_7_1
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing Entity element
    ...    ${expectation_filename}
    ...    ${entity_id}
    ...    ${response.json()}
    ...    ${True}


*** Keywords ***
Create Entity
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}

Delete Created Entity
    Delete Entity    ${entity_id}

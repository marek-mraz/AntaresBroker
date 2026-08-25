*** Settings ***
Documentation       Check that one can query one entity via POST Interaction based on id

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Entities
Suite Teardown      Delete Entities


*** Variables ***
${filename}=                building-minimal.jsonld
${expectation_filename}=    building-minimal.json
${entity_type}=             https://ngsi-ld-test-suite/context#Building


*** Test Cases ***
019_02_01 Query One Entity Via POST Interaction Based On Id
    [Documentation]    Check that one can query one entity via POST Interaction based on id
    [Tags]    e-query    5_7_2
    @{entities_ids_to_be_compared}=    Create List    ${first_entity_id}
    ${entity_selector}=    Create Dictionary    id=${first_entity_id}    type=${entity_type}
    @{entities}=    Create List    ${entity_selector}
    ${response}=    Query Entities Via POST
    ...    entities=${entities}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Entity Elements
    ...    ${expectation_filename}
    ...    ${entities_ids_to_be_compared}
    ...    ${response.json()}


*** Keywords ***
Create Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${first_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${second_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${second_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${third_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${third_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${third_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
    Delete Entity    ${third_entity_id}

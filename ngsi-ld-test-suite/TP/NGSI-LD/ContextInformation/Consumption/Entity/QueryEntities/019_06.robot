*** Settings ***
Documentation       Check that one can query entities specifying a maximum number of results

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Entities
Suite Teardown      Delete Entities


*** Variables ***
${filename}=        building-minimal.jsonld
${entity_type}=     https://ngsi-ld-test-suite/context#Building
${limit}=           2


*** Test Cases ***
019_06_01 Query Entities Specifying A Maximum Number Of Results
    [Documentation]    Check that one can query entities specifying a maximum number of results
    [Tags]    e-query    6_3_10
    ${entities_ids_to_be_retrieved}=    Catenate
    ...    SEPARATOR=,
    ...    ${first_entity_id}
    ...    ${second_entity_id}
    ...    ${third_entity_id}
    ${entity_types_to_be_retrieved}=    Catenate    SEPARATOR=,    ${entity_type}

    ${response}=    Query Entities
    ...    entity_ids=${entities_ids_to_be_retrieved}
    ...    entity_types=${entity_types_to_be_retrieved}
    ...    limit=${limit}

    Check Response Status Code    200    ${response.status_code}
    @{entities_ids_to_be_compared}=    Create List    ${first_entity_id}    ${second_entity_id}
    Check Response Body Containing Number Of Entities    ${entity_type}    ${2}    ${response.json()}


*** Keywords ***
Create Entities
    ${first_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${first_entity_id}
    ${create_response1}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${first_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response1.status_code}
    ${second_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${second_entity_id}
    ${create_response2}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}
    ${third_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${third_entity_id}
    ${create_response3}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${third_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response3.status_code}

Delete Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
    Delete Entity    ${third_entity_id}

*** Settings ***
Documentation       Check that one can query several entities via POST Interaction based on the entity type

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Entities
Suite Teardown      Delete Entities


*** Variables ***
${building_filename}=       building-minimal.jsonld
${vehicle_filename}=        vehicle-simple-attributes.jsonld
${expectation_filename}=    two-vehicles.jsonld
${building_entity_type}=    https://ngsi-ld-test-suite/context#Building
${vehicle_entity_type}=     https://ngsi-ld-test-suite/context#Vehicle


*** Test Cases ***
019_02_02 Query Several Entities Via POST Interaction Based On The Entities Types
    [Documentation]    Check that one can query several entities via POST Interaction based on the entity type
    [Tags]    e-query    5_7_2
    ${entities_ids_to_be_compared}=    Create List    ${first_vehicle_entity_id}    ${second_vehicle_entity_id}
    ${entity_selector}=    Create Dictionary    type=${vehicle_entity_type}
    @{entities}=    Create List    ${entity_selector}
    ${response}=    Query Entities Via POST
    ...    entities=${entities}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Entity Elements With Different Types
    ...    filename=${expectation_filename}
    ...    entities_representation_ids=${entities_ids_to_be_compared}
    ...    response_body=${response.json()}
    ...    ignore_core_context_version=True


*** Keywords ***
Create Entities
    ${building_entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${building_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${building_filename}
    ...    ${building_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${first_vehicle_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${first_vehicle_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${vehicle_filename}
    ...    ${first_vehicle_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    ${second_vehicle_entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${second_vehicle_entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${vehicle_filename}
    ...    ${second_vehicle_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}

Delete Entities
    Delete Entity    ${building_entity_id}
    Delete Entity    ${first_vehicle_entity_id}
    Delete Entity    ${second_vehicle_entity_id}

*** Settings ***
Documentation       Check that one can query several entities based on attribute names

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Entities


*** Variables ***
${filename}=                        building-simple-attributes.jsonld
${filename2}=                       building-minimal.jsonld
${expectation_filename}=            building-attributes-query.json
${entity_type}=                     https://ngsi-ld-test-suite/context#Building
${attribute_airqualitylevel}=       https://ngsi-ld-test-suite/context#airQualityLevel
${attribute_subcategory}=           https://ngsi-ld-test-suite/context#subCategory


*** Test Cases ***
019_01_04 Query Several Entities Based On Attribute Names
    [Documentation]    Check that one can query several entities based on attribute names
    [Tags]    e-query    5_7_2
    ${attributes_to_be_retrieved}=    Catenate
    ...    SEPARATOR=,
    ...    ${attribute_airqualitylevel}
    ...    ${attribute_subcategory}
    @{entities_ids_to_be_compared}=    Create List    ${first_entity_id}

    ${response}=    Query Entities    attrs=${attributes_to_be_retrieved}

    Check Response Status Code    200    ${response.status_code}
    Check Response Body Containing List Containing Entity Elements
    ...    ${expectation_filename}
    ...    ${entities_ids_to_be_compared}
    ...    ${response.json()}


*** Keywords ***
Setup Initial Entities
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
    ...    ${filename2}
    ...    ${second_entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response2.status_code}

Delete Entities
    Delete Entity    ${first_entity_id}
    Delete Entity    ${second_entity_id}
